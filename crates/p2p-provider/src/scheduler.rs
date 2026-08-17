use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use p2p_domain::{
    CalculationError, EligibilityFilters, ExactDecimal, MarketPair, PageReceiptTiming,
    PaymentLogic, PaymentMethod, RequestedAmount, ResultsTarget, SideQuality, StableId, UserIntent,
    evaluate_eligibility,
};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::circuit::{CircuitBreaker, CircuitReason, CircuitState};
use crate::contract::{
    ContractError, NormalizedAd, RecordRejectionCode, ValidatedPage, WebSearchPageRequest,
    validate_web_search_page,
};
use crate::policy::{
    FIRST_RETRY_DELAY, MAX_ATTEMPTS, MAX_PAGES_PER_SIDE, PAGE_SIZE, SECOND_RETRY_DELAY,
};
use crate::transport::{
    GlobalRequestGate, PageTransport, TransportError, TransportRequest, TransportResponse,
};

#[derive(Clone, Debug)]
pub struct AcquisitionEligibility {
    pub amount: RequestedAmount,
    pub filters: EligibilityFilters,
}

#[derive(Clone, Debug)]
pub struct AcquisitionRequest {
    pub request_id: StableId,
    pub pair: MarketPair,
    pub transaction_amount: Option<ExactDecimal>,
    pub selected_payment_methods: BTreeSet<PaymentMethod>,
    pub payment_logic: PaymentLogic,
    pub target: ResultsTarget,
    pub local_eligibility: Option<AcquisitionEligibility>,
}

impl AcquisitionRequest {
    fn upstream_payment_methods(&self) -> BTreeSet<PaymentMethod> {
        if self.selected_payment_methods.len() == 1 {
            self.selected_payment_methods.clone()
        } else {
            BTreeSet::new()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressStage {
    Queued,
    WaitingForRateLimit,
    Requesting,
    BackingOff,
    Validating,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideProgress {
    pub next_page: u8,
    pub fetched: u32,
    pub valid: u32,
    pub duplicates: u32,
    pub rejected: u32,
    pub target: u32,
    pub provider_total: Option<u32>,
    pub exhausted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionProgress {
    pub stage: ProgressStage,
    pub active_intent: Option<UserIntent>,
    pub attempts_for_page: u8,
    pub requests_completed: u16,
    pub buy: SideProgress,
    pub sell: SideProgress,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideAcquisition {
    pub ads: Vec<NormalizedAd>,
    pub quality: SideQuality,
    pub rejection_counts: BTreeMap<RecordRejectionCode, u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Acquisition {
    pub request_id: StableId,
    pub pair: MarketPair,
    pub buy: SideAcquisition,
    pub sell: SideAcquisition,
    pub page_receipts: Vec<PageReceiptTiming>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaginationFailure {
    InconsistentTerminalPage,
    RepeatedOrNoProgressPage,
    PageBudgetExhausted,
    FetchedExceedsTotal,
    AllRowsRejected,
    AsymmetricProviderZero,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderError {
    #[error("provider acquisition was cancelled")]
    Cancelled,
    #[error("provider circuit is open")]
    CircuitOpen(CircuitState),
    #[error("provider HTTP request failed with status {0}")]
    Http(u16),
    #[error("provider transport failed")]
    Transport(TransportError),
    #[error("provider contract validation failed")]
    Contract(ContractError),
    #[error("provider pagination validation failed")]
    Pagination(PaginationFailure),
    #[error("local eligibility calculation failed")]
    Eligibility(CalculationError),
    #[error("provider quality counters were internally inconsistent")]
    Quality,
}

#[derive(Clone, Copy, Debug)]
struct RetryTiming {
    first: Duration,
    second: Duration,
}

impl RetryTiming {
    const PRODUCTION: Self = Self {
        first: FIRST_RETRY_DELAY,
        second: SECOND_RETRY_DELAY,
    };

    #[cfg(test)]
    const FAST_TEST: Self = Self {
        first: Duration::from_millis(1),
        second: Duration::from_millis(2),
    };
}

pub struct ProviderService<T> {
    transport: Arc<T>,
    request_gate: GlobalRequestGate,
    circuit: CircuitBreaker,
    graph_lock: Arc<Mutex<()>>,
    retry_timing: RetryTiming,
}

impl<T> Clone for ProviderService<T> {
    fn clone(&self) -> Self {
        Self {
            transport: Arc::clone(&self.transport),
            request_gate: self.request_gate.clone(),
            circuit: self.circuit.clone(),
            graph_lock: Arc::clone(&self.graph_lock),
            retry_timing: self.retry_timing,
        }
    }
}

impl<T: PageTransport> ProviderService<T> {
    pub fn new(transport: T) -> Self {
        Self::with_shared(
            transport,
            GlobalRequestGate::production(),
            CircuitBreaker::new(),
        )
    }

    pub(crate) fn with_shared(
        transport: T,
        request_gate: GlobalRequestGate,
        circuit: CircuitBreaker,
    ) -> Self {
        Self {
            transport: Arc::new(transport),
            request_gate,
            circuit,
            graph_lock: Arc::new(Mutex::new(())),
            retry_timing: RetryTiming::PRODUCTION,
        }
    }

    #[cfg(test)]
    fn new_for_test(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            request_gate: GlobalRequestGate::with_gap(Duration::ZERO),
            circuit: CircuitBreaker::new(),
            graph_lock: Arc::new(Mutex::new(())),
            retry_timing: RetryTiming::FAST_TEST,
        }
    }

    pub fn circuit(&self) -> &CircuitBreaker {
        &self.circuit
    }

    pub async fn acquire<F>(
        &self,
        request: AcquisitionRequest,
        cancellation: CancellationToken,
        on_progress: F,
    ) -> Result<Acquisition, ProviderError>
    where
        F: Fn(AcquisitionProgress) + Send + Sync,
    {
        self.circuit
            .ensure_available()
            .await
            .map_err(ProviderError::CircuitOpen)?;
        let target = u32::from(request.target.value());
        let mut buy = SideAccumulator::new(UserIntent::BuyAsset, target);
        let mut sell = SideAccumulator::new(UserIntent::SellAsset, target);
        on_progress(progress(ProgressStage::Queued, None, 0, 0, &buy, &sell));
        let _graph = tokio::select! {
            () = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            guard = self.graph_lock.lock() => guard,
        };
        self.circuit
            .ensure_available()
            .await
            .map_err(ProviderError::CircuitOpen)?;

        let mut requests_completed = 0_u16;
        let mut page_receipts = Vec::new();
        loop {
            if buy.done && sell.done {
                break;
            }
            for intent in [UserIntent::BuyAsset, UserIntent::SellAsset] {
                let (done, next_page) = match intent {
                    UserIntent::BuyAsset => (buy.done, buy.next_page),
                    UserIntent::SellAsset => (sell.done, sell.next_page),
                };
                if done {
                    continue;
                }
                if cancellation.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                if next_page > MAX_PAGES_PER_SIDE {
                    return Err(ProviderError::Pagination(
                        PaginationFailure::PageBudgetExhausted,
                    ));
                }
                let payload = WebSearchPageRequest::new(
                    &request.pair,
                    intent,
                    next_page,
                    request.transaction_amount,
                    &request.upstream_payment_methods(),
                )
                .map_err(ProviderError::Contract)?;
                on_progress(progress(
                    ProgressStage::Requesting,
                    Some(intent),
                    1,
                    requests_completed,
                    &buy,
                    &sell,
                ));
                let (response, attempts_used) = self
                    .fetch_with_retry(
                        payload,
                        intent,
                        &request.request_id,
                        &cancellation,
                        |attempt, stage| {
                            on_progress(progress(
                                stage,
                                Some(intent),
                                attempt,
                                requests_completed,
                                &buy,
                                &sell,
                            ));
                        },
                    )
                    .await?;
                requests_completed = requests_completed.saturating_add(u16::from(attempts_used));
                on_progress(progress(
                    ProgressStage::Validating,
                    Some(intent),
                    0,
                    requests_completed,
                    &buy,
                    &sell,
                ));
                let validated = match validate_web_search_page(
                    &response.body,
                    &request.pair,
                    intent,
                    response.received_at_ms,
                ) {
                    Ok(page) => page,
                    Err(error) => {
                        self.open_contract_circuit(&error).await;
                        return Err(ProviderError::Contract(error));
                    }
                };
                page_receipts.push(
                    PageReceiptTiming::new(intent, u16::from(next_page), response.received_at_ms)
                        .map_err(|_| ProviderError::Quality)?,
                );
                match intent {
                    UserIntent::BuyAsset => {
                        buy.apply_page(validated, request.local_eligibility.as_ref())?
                    }
                    UserIntent::SellAsset => {
                        sell.apply_page(validated, request.local_eligibility.as_ref())?
                    }
                }
            }
        }

        if (buy.provider_total == Some(0)) ^ (sell.provider_total == Some(0)) {
            return Err(ProviderError::Pagination(
                PaginationFailure::AsymmetricProviderZero,
            ));
        }
        for side in [&buy, &sell] {
            if side.ads.is_empty() && side.fetched > 0 && side.exhausted && side.local_rejected == 0
            {
                return Err(ProviderError::Pagination(
                    PaginationFailure::AllRowsRejected,
                ));
            }
        }
        on_progress(progress(
            ProgressStage::Complete,
            None,
            0,
            requests_completed,
            &buy,
            &sell,
        ));
        Ok(Acquisition {
            request_id: request.request_id,
            pair: request.pair,
            buy: buy.finish()?,
            sell: sell.finish()?,
            page_receipts,
        })
    }

    async fn fetch_with_retry<F>(
        &self,
        payload: WebSearchPageRequest,
        intent: UserIntent,
        request_id: &StableId,
        cancellation: &CancellationToken,
        on_attempt: F,
    ) -> Result<(TransportResponse, u8), ProviderError>
    where
        F: Fn(u8, ProgressStage),
    {
        let mut last_error = None;
        for attempt in 1..=MAX_ATTEMPTS {
            self.circuit
                .ensure_available()
                .await
                .map_err(ProviderError::CircuitOpen)?;
            on_attempt(attempt, ProgressStage::WaitingForRateLimit);
            let transport_request = TransportRequest {
                payload: payload.clone(),
            };
            let result = self
                .request_gate
                .run(
                    cancellation,
                    self.transport.send(transport_request, cancellation.clone()),
                )
                .await;
            match result {
                Ok(response) if response.status == 200 => return Ok((response, attempt)),
                Ok(response) if response.status == 429 => {
                    let state = self.circuit.open_rate_limit(response.retry_after).await;
                    return Err(ProviderError::CircuitOpen(state));
                }
                Ok(response) if matches!(response.status, 403 | 418) => {
                    let state = self.circuit.open_waf_or_ban(response.retry_after).await;
                    return Err(ProviderError::CircuitOpen(state));
                }
                Ok(response) if is_retryable_status(response.status) => {
                    last_error = Some(ProviderError::Http(response.status));
                }
                Ok(response) => return Err(ProviderError::Http(response.status)),
                Err(TransportError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(error) if is_retryable_transport(error) => {
                    last_error = Some(ProviderError::Transport(error));
                }
                Err(error) => return Err(ProviderError::Transport(error)),
            }
            if attempt < MAX_ATTEMPTS {
                on_attempt(attempt, ProgressStage::BackingOff);
                let delay = jittered_delay(
                    if attempt == 1 {
                        self.retry_timing.first
                    } else {
                        self.retry_timing.second
                    },
                    request_id.as_str(),
                    intent,
                    payload.page(),
                    attempt,
                );
                tokio::select! {
                    () = cancellation.cancelled() => return Err(ProviderError::Cancelled),
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
        Err(last_error.unwrap_or(ProviderError::Transport(TransportError::Network)))
    }

    async fn open_contract_circuit(&self, error: &ContractError) {
        match error {
            ContractError::WrongSide | ContractError::CrossPair => {
                self.circuit
                    .open_persistent(CircuitReason::SideContract)
                    .await;
            }
            ContractError::InvalidEnvelope | ContractError::InvalidTotal => {
                self.circuit
                    .open_persistent(CircuitReason::SchemaContract)
                    .await;
            }
            ContractError::InvalidPage | ContractError::ProviderRejected => {}
        }
    }
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 500 | 502 | 503 | 504)
}

fn is_retryable_transport(error: TransportError) -> bool {
    matches!(
        error,
        TransportError::ConnectTimeout | TransportError::RequestTimeout | TransportError::Network
    )
}

fn jittered_delay(
    base: Duration,
    request_id: &str,
    intent: UserIntent,
    page: u8,
    attempt: u8,
) -> Duration {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in request_id.bytes().chain([intent as u8, page, attempt]) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let percent = 90_u128 + u128::from(hash % 21);
    let millis = base.as_millis().saturating_mul(percent) / 100;
    Duration::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

struct SideAccumulator {
    intent: UserIntent,
    next_page: u8,
    fetched: u32,
    duplicates: u32,
    rejected: u32,
    local_rejected: u32,
    target: u32,
    provider_total: Option<u32>,
    exhausted: bool,
    done: bool,
    ads: Vec<NormalizedAd>,
    seen_ids: BTreeSet<String>,
    rejection_counts: BTreeMap<RecordRejectionCode, u32>,
}

impl SideAccumulator {
    fn new(intent: UserIntent, target: u32) -> Self {
        Self {
            intent,
            next_page: 1,
            fetched: 0,
            duplicates: 0,
            rejected: 0,
            local_rejected: 0,
            target,
            provider_total: None,
            exhausted: false,
            done: false,
            ads: Vec::new(),
            seen_ids: BTreeSet::new(),
            rejection_counts: BTreeMap::new(),
        }
    }

    fn progress(&self) -> SideProgress {
        SideProgress {
            next_page: self.next_page,
            fetched: self.fetched,
            valid: u32::try_from(self.ads.len()).unwrap_or(u32::MAX),
            duplicates: self.duplicates,
            rejected: self.rejected,
            target: self.target,
            provider_total: self.provider_total,
            exhausted: self.exhausted,
        }
    }

    fn apply_page(
        &mut self,
        page: ValidatedPage,
        local_eligibility: Option<&AcquisitionEligibility>,
    ) -> Result<(), ProviderError> {
        let fetched_after = self.fetched.saturating_add(page.fetched);
        if fetched_after > page.provider_total {
            return Err(ProviderError::Pagination(
                PaginationFailure::FetchedExceedsTotal,
            ));
        }
        self.fetched = fetched_after;
        self.provider_total = Some(page.provider_total);
        self.rejected = self
            .rejected
            .saturating_add(u32::try_from(page.rejections.len()).unwrap_or(u32::MAX));
        for rejection in page.rejections {
            self.rejection_counts
                .entry(rejection.code)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }

        let mut unique_on_page = 0_u32;
        for normalized in page.ads {
            let id = normalized.ad.stable_id().as_str().to_owned();
            if !self.seen_ids.insert(id) {
                self.duplicates = self.duplicates.saturating_add(1);
                continue;
            }
            unique_on_page = unique_on_page.saturating_add(1);
            if let Some(eligibility) = local_eligibility {
                let evaluation = evaluate_eligibility(
                    self.intent,
                    eligibility.amount,
                    &eligibility.filters,
                    &normalized.ad,
                )
                .map_err(ProviderError::Eligibility)?;
                if !evaluation.eligible() {
                    self.local_rejected = self.local_rejected.saturating_add(1);
                    self.rejected = self.rejected.saturating_add(1);
                    continue;
                }
            }
            if self.ads.len() < self.target as usize {
                self.ads.push(normalized);
            }
        }
        if page.fetched == u32::from(PAGE_SIZE)
            && unique_on_page == 0
            && self.ads.len() < self.target as usize
        {
            return Err(ProviderError::Pagination(
                PaginationFailure::RepeatedOrNoProgressPage,
            ));
        }
        if self.ads.len() >= self.target as usize {
            self.done = true;
            return Ok(());
        }

        let provider_exhausted = self.fetched >= page.provider_total;
        if page.fetched < u32::from(PAGE_SIZE) {
            if !provider_exhausted {
                return Err(ProviderError::Pagination(
                    PaginationFailure::InconsistentTerminalPage,
                ));
            }
            self.exhausted = true;
            self.done = true;
            return Ok(());
        }
        if provider_exhausted {
            self.exhausted = true;
            self.done = true;
            return Ok(());
        }
        self.next_page = self.next_page.saturating_add(1);
        Ok(())
    }

    fn finish(self) -> Result<SideAcquisition, ProviderError> {
        let valid = u32::try_from(self.ads.len()).map_err(|_| ProviderError::Quality)?;
        let quality = SideQuality::new(
            self.fetched,
            valid,
            self.duplicates,
            self.rejected,
            self.target,
            self.provider_total,
            self.exhausted,
        )
        .map_err(|_| ProviderError::Quality)?;
        Ok(SideAcquisition {
            ads: self.ads,
            quality,
            rejection_counts: self.rejection_counts,
        })
    }
}

fn progress(
    stage: ProgressStage,
    active_intent: Option<UserIntent>,
    attempts_for_page: u8,
    requests_completed: u16,
    buy: &SideAccumulator,
    sell: &SideAccumulator,
) -> AcquisitionProgress {
    debug_assert_eq!(buy.intent, UserIntent::BuyAsset);
    debug_assert_eq!(sell.intent, UserIntent::SellAsset);
    AcquisitionProgress {
        stage,
        active_intent,
        attempts_for_page,
        requests_completed,
        buy: buy.progress(),
        sell: sell.progress(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::str::FromStr;

    use p2p_domain::{RequestSide, Symbol};
    use serde_json::{Value, json};

    use super::*;
    use crate::transport::{TransportFuture, TransportRequest};

    #[derive(Clone)]
    struct MockTransport {
        responses: Arc<Mutex<VecDeque<Result<TransportResponse, TransportError>>>>,
        requests: Arc<Mutex<Vec<(RequestSide, u8)>>>,
    }

    impl MockTransport {
        fn new(responses: Vec<Result<TransportResponse, TransportError>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into())),
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl PageTransport for MockTransport {
        fn send<'a>(
            &'a self,
            request: TransportRequest,
            _cancellation: CancellationToken,
        ) -> TransportFuture<'a> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .await
                    .push((request.payload.trade_type(), request.payload.page()));
                self.responses
                    .lock()
                    .await
                    .pop_front()
                    .expect("mock response")
            })
        }
    }

    fn pair() -> MarketPair {
        MarketPair::new(
            Symbol::new("USDT").expect("asset"),
            Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair")
    }

    fn request() -> AcquisitionRequest {
        AcquisitionRequest {
            request_id: StableId::new("synthetic-request-1").expect("id"),
            pair: pair(),
            transaction_amount: Some(ExactDecimal::from_str("10000").expect("amount")),
            selected_payment_methods: BTreeSet::new(),
            payment_logic: PaymentLogic::Any,
            target: ResultsTarget::new(20).expect("target"),
            local_eligibility: None,
        }
    }

    fn row(index: u8, advertiser_side: &str) -> Value {
        json!({
            "adv": {
                "advNo": format!("synthetic-ad-{advertiser_side}-{index}"),
                "tradeType": advertiser_side,
                "asset": "USDT",
                "fiatUnit": "EGP",
                "price": "50",
                "minSingleTransAmount": "100",
                "maxSingleTransAmount": "10000",
                "dynamicMaxSingleTransAmount": "10000",
                "tradableQuantity": "100",
                "tradeMethods": [{"identifier": "SYNTHETIC_PAY"}],
                "isTradable": true
            },
            "advertiser": {
                "userNo": format!("synthetic-merchant-{index}"),
                "nickName": format!("Synthetic {index}"),
                "monthOrderCount": 100,
                "monthFinishRate": "0.99",
                "positiveRate": "1",
                "merchantGroupMember": false,
                "activeTimeInSecond": 10
            }
        })
    }

    fn response(side: &str, total: u32, count: u8) -> TransportResponse {
        response_range(side, total, 0, count)
    }

    fn response_range(side: &str, total: u32, start: u8, count: u8) -> TransportResponse {
        let data = (start..start.saturating_add(count))
            .map(|index| row(index, side))
            .collect::<Vec<_>>();
        TransportResponse {
            status: 200,
            body: serde_json::to_vec(&json!({
                "code": "000000", "success": true, "total": total, "data": data
            }))
            .expect("body"),
            retry_after: None,
            received_at_ms: 100 + i64::from(count),
        }
    }

    #[tokio::test]
    async fn alternates_corrected_sides_and_accepts_trustworthy_exhaustion() {
        let transport =
            MockTransport::new(vec![Ok(response("SELL", 1, 1)), Ok(response("BUY", 1, 1))]);
        let requests = Arc::clone(&transport.requests);
        let service = ProviderService::new_for_test(transport);
        let result = service
            .acquire(request(), CancellationToken::new(), |_| {})
            .await
            .expect("complete acquisition");
        assert_eq!(result.buy.ads.len(), 1);
        assert_eq!(result.sell.ads.len(), 1);
        assert!(result.buy.quality.exhausted());
        assert_eq!(
            *requests.lock().await,
            vec![(RequestSide::Buy, 1), (RequestSide::Sell, 1)]
        );
    }

    #[tokio::test]
    async fn retries_only_transient_failures_and_never_returns_partial_data() {
        let transport = MockTransport::new(vec![
            Err(TransportError::RequestTimeout),
            Ok(response("SELL", 1, 1)),
            Ok(response("BUY", 1, 1)),
        ]);
        let requests = Arc::clone(&transport.requests);
        let service = ProviderService::new_for_test(transport);
        let result = service
            .acquire(request(), CancellationToken::new(), |_| {})
            .await
            .expect("retry succeeds");
        assert_eq!(result.buy.ads.len(), 1);
        assert_eq!(requests.lock().await.len(), 3);

        let failing = ProviderService::new_for_test(MockTransport::new(vec![
            Ok(response("SELL", 1, 1)),
            Ok(TransportResponse {
                status: 400,
                body: Vec::new(),
                retry_after: None,
                received_at_ms: 2,
            }),
        ]));
        assert_eq!(
            failing
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Http(400))
        );
    }

    #[tokio::test]
    async fn rate_limit_and_wrong_side_open_visible_circuits() {
        let limited =
            ProviderService::new_for_test(MockTransport::new(vec![Ok(TransportResponse {
                status: 429,
                body: Vec::new(),
                retry_after: Some(Duration::from_secs(120)),
                received_at_ms: 1,
            })]));
        assert!(matches!(
            limited
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::CircuitOpen(CircuitState::Timed {
                reason: CircuitReason::RateLimited,
                ..
            }))
        ));

        let wrong =
            ProviderService::new_for_test(MockTransport::new(vec![Ok(response("BUY", 1, 1))]));
        assert_eq!(
            wrong
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Contract(ContractError::WrongSide))
        );
        assert_eq!(
            wrong.circuit().state().await,
            CircuitState::Persistent {
                reason: CircuitReason::SideContract
            }
        );
    }

    #[tokio::test]
    async fn cancellation_interrupts_queue_or_backoff_without_fallback() {
        let transport = MockTransport::new(vec![Err(TransportError::RequestTimeout)]);
        let service = ProviderService::new_for_test(transport);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            service.acquire(request(), cancellation, |_| {}).await,
            Err(ProviderError::Cancelled)
        );
    }

    #[tokio::test]
    async fn repeated_full_page_and_inconsistent_short_page_fail_closed() {
        let mut target_40 = request();
        target_40.target = ResultsTarget::new(40).expect("target");
        let repeated = ProviderService::new_for_test(MockTransport::new(vec![
            Ok(response("SELL", 40, 20)),
            Ok(response("BUY", 40, 20)),
            Ok(response("SELL", 40, 20)),
        ]));
        assert_eq!(
            repeated
                .acquire(target_40, CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Pagination(
                PaginationFailure::RepeatedOrNoProgressPage
            ))
        );

        let short =
            ProviderService::new_for_test(MockTransport::new(vec![Ok(response("SELL", 10, 1))]));
        assert_eq!(
            short
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Pagination(
                PaginationFailure::InconsistentTerminalPage
            ))
        );
    }

    #[tokio::test]
    async fn provider_zero_is_distinct_from_asymmetric_zero_and_all_rows_rejected() {
        let empty = ProviderService::new_for_test(MockTransport::new(vec![
            Ok(response("SELL", 0, 0)),
            Ok(response("BUY", 0, 0)),
        ]));
        let result = empty
            .acquire(request(), CancellationToken::new(), |_| {})
            .await
            .expect("confirmed empty market");
        assert!(result.buy.ads.is_empty());
        assert!(result.sell.ads.is_empty());
        assert!(result.buy.quality.exhausted());

        let asymmetric = ProviderService::new_for_test(MockTransport::new(vec![
            Ok(response("SELL", 0, 0)),
            Ok(response("BUY", 1, 1)),
        ]));
        assert_eq!(
            asymmetric
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Pagination(
                PaginationFailure::AsymmetricProviderZero
            ))
        );

        let mut rejected_sell = row(0, "SELL");
        rejected_sell["adv"]["price"] = json!("NaN");
        let mut rejected_buy = row(0, "BUY");
        rejected_buy["adv"]["price"] = json!("NaN");
        let rejected_response = |value: Value| TransportResponse {
            status: 200,
            body: serde_json::to_vec(&json!({
                "code": "000000", "success": true, "total": 1, "data": [value]
            }))
            .expect("body"),
            retry_after: None,
            received_at_ms: 100,
        };
        let rejected = ProviderService::new_for_test(MockTransport::new(vec![
            Ok(rejected_response(rejected_sell)),
            Ok(rejected_response(rejected_buy)),
        ]));
        assert_eq!(
            rejected
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Pagination(
                PaginationFailure::AllRowsRejected
            ))
        );
    }

    #[tokio::test]
    async fn retries_stop_after_three_attempts_and_waf_opens_long_circuit() {
        let transport = MockTransport::new(vec![
            Ok(TransportResponse {
                status: 503,
                body: Vec::new(),
                retry_after: None,
                received_at_ms: 1,
            });
            3
        ]);
        let requests = Arc::clone(&transport.requests);
        let exhausted = ProviderService::new_for_test(transport);
        assert_eq!(
            exhausted
                .acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::Http(503))
        );
        assert_eq!(requests.lock().await.len(), 3);

        let waf = ProviderService::new_for_test(MockTransport::new(vec![Ok(TransportResponse {
            status: 403,
            body: Vec::new(),
            retry_after: None,
            received_at_ms: 1,
        })]));
        assert!(matches!(
            waf.acquire(request(), CancellationToken::new(), |_| {})
                .await,
            Err(ProviderError::CircuitOpen(CircuitState::Timed {
                reason: CircuitReason::WafOrBan,
                ..
            }))
        ));
    }

    #[tokio::test]
    async fn local_eligibility_continues_paging_to_trustworthy_exhaustion() {
        use p2p_domain::{AmountMode, EligibilityFiltersInput, RequestedAmount};

        let transport = MockTransport::new(vec![
            Ok(response("SELL", 21, 20)),
            Ok(response("BUY", 21, 20)),
            Ok(response_range("SELL", 21, 20, 1)),
            Ok(response_range("BUY", 21, 20, 1)),
        ]);
        let requests = Arc::clone(&transport.requests);
        let service = ProviderService::new_for_test(transport);
        let mut value = request();
        value.local_eligibility = Some(AcquisitionEligibility {
            amount: RequestedAmount::new(ExactDecimal::from_i64(10_000), AmountMode::Fiat)
                .expect("amount"),
            filters: EligibilityFilters::new(EligibilityFiltersInput {
                selected_payments: BTreeSet::new(),
                payment_logic: PaymentLogic::Any,
                minimum_orders: 200,
                minimum_completion_percent: ExactDecimal::ZERO,
                minimum_positive_percent: ExactDecimal::ZERO,
                pro_only: false,
                maximum_buy_price: None,
                minimum_sell_price: None,
            })
            .expect("filters"),
        });
        let result = service
            .acquire(value, CancellationToken::new(), |_| {})
            .await
            .expect("confirmed local no-match");
        assert!(result.buy.ads.is_empty());
        assert!(result.sell.ads.is_empty());
        assert!(result.buy.quality.exhausted());
        assert_eq!(result.buy.quality.rejected(), 21);
        assert_eq!(requests.lock().await.len(), 4);
    }

    #[test]
    fn upstream_payment_filter_is_used_only_when_single_and_unambiguous() {
        let mut value = request();
        value
            .selected_payment_methods
            .insert(PaymentMethod::new("SYNTHETIC_A").expect("method"));
        assert_eq!(value.upstream_payment_methods().len(), 1);
        value
            .selected_payment_methods
            .insert(PaymentMethod::new("SYNTHETIC_B").expect("method"));
        value.payment_logic = PaymentLogic::All;
        assert!(value.upstream_payment_methods().is_empty());
    }

    #[test]
    fn jitter_stays_within_ten_percent_without_binary_float() {
        for attempt in 1..=2 {
            let value = jittered_delay(
                Duration::from_secs(1),
                "synthetic-request",
                UserIntent::BuyAsset,
                1,
                attempt,
            );
            assert!((Duration::from_millis(900)..=Duration::from_millis(1100)).contains(&value));
        }
    }
}
