#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use p2p_domain::{
    AmountMode, EligibilityFilters, EligibilityFiltersInput, ExactDecimal, MarketPair,
    PaymentLogic, PaymentMethod, RequestedAmount, ResultsTarget, StableId, Symbol, UserIntent,
    evaluate_eligibility, rank_offers,
};
use p2p_persistence::SnapshotContext;
use p2p_provider::{Acquisition, SideAcquisition};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_AUTO_REFRESH: bool = true;
pub const DEFAULT_REFRESH_INTERVAL_SECONDS: u32 = 20;
pub const MIN_REFRESH_INTERVAL_SECONDS: u32 = 10;
pub const MAX_REFRESH_INTERVAL_SECONDS: u32 = 3_600;
pub const SETTINGS_SECTION: &str = "lifecycle";
pub const SETTINGS_KEY: &str = "state-v1";
pub const PERSISTED_LIFECYCLE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedLifecycle {
    pub version: u32,
    pub settings: RefreshSettings,
    pub draft: MarketContextDraft,
    pub applied: MarketContextDraft,
    pub last_success_ms: Option<i64>,
}

impl Default for PersistedLifecycle {
    fn default() -> Self {
        let context = MarketContextDraft::first_run_default();
        Self {
            version: PERSISTED_LIFECYCLE_VERSION,
            settings: RefreshSettings::default(),
            draft: context.clone(),
            applied: context,
            last_success_ms: None,
        }
    }
}

impl PersistedLifecycle {
    pub fn validate(&self) -> Result<(), LifecycleError> {
        if self.version != PERSISTED_LIFECYCLE_VERSION {
            return Err(LifecycleError::UnsupportedPersistedVersion(self.version));
        }
        self.settings.validate()?;
        self.draft.validate()?;
        self.applied.validate()?;
        if self.last_success_ms.is_some_and(|timestamp| timestamp < 0) {
            return Err(LifecycleError::InvalidLastSuccessTimestamp);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSettings {
    pub auto_refresh: bool,
    pub interval_seconds: u32,
}

impl Default for RefreshSettings {
    fn default() -> Self {
        Self {
            auto_refresh: DEFAULT_AUTO_REFRESH,
            interval_seconds: DEFAULT_REFRESH_INTERVAL_SECONDS,
        }
    }
}

impl RefreshSettings {
    pub fn validate(self) -> Result<Self, LifecycleError> {
        if !(MIN_REFRESH_INTERVAL_SECONDS..=MAX_REFRESH_INTERVAL_SECONDS)
            .contains(&self.interval_seconds)
        {
            return Err(LifecycleError::InvalidRefreshInterval {
                minimum: MIN_REFRESH_INTERVAL_SECONDS,
                maximum: MAX_REFRESH_INTERVAL_SECONDS,
                supplied: self.interval_seconds,
            });
        }
        Ok(self)
    }

    pub fn interval_ms(self) -> i64 {
        i64::from(self.interval_seconds) * 1_000
    }

    pub fn stale_after_ms(self) -> i64 {
        60_000_i64.max(self.interval_ms().saturating_mul(2))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketContextDraft {
    pub asset: String,
    pub fiat: String,
    pub amount: String,
    pub amount_mode: AmountMode,
    pub selected_payment_methods: BTreeSet<String>,
    pub payment_logic: PaymentLogic,
    pub minimum_orders: u64,
    pub minimum_completion_percent: String,
    pub minimum_positive_percent: String,
    pub pro_only: bool,
    pub maximum_buy_price: Option<String>,
    pub minimum_sell_price: Option<String>,
    pub results_target: u16,
}

impl MarketContextDraft {
    pub fn first_run_default() -> Self {
        Self {
            asset: "USDT".to_owned(),
            fiat: "EGP".to_owned(),
            amount: "10000".to_owned(),
            amount_mode: AmountMode::Fiat,
            selected_payment_methods: BTreeSet::new(),
            payment_logic: PaymentLogic::Any,
            minimum_orders: 0,
            minimum_completion_percent: "0".to_owned(),
            minimum_positive_percent: "0".to_owned(),
            pro_only: false,
            maximum_buy_price: None,
            minimum_sell_price: None,
            results_target: 40,
        }
    }

    pub fn validate(&self) -> Result<ValidatedContext, LifecycleError> {
        let pair = MarketPair::new(
            Symbol::new(&self.asset).map_err(|error| LifecycleError::InvalidContext {
                field: "asset",
                detail: error.to_string(),
            })?,
            Symbol::new(&self.fiat).map_err(|error| LifecycleError::InvalidContext {
                field: "fiat",
                detail: error.to_string(),
            })?,
        )
        .map_err(|error| LifecycleError::InvalidContext {
            field: "pair",
            detail: error.to_string(),
        })?;
        let amount_value = decimal("amount", &self.amount)?;
        let amount = RequestedAmount::new(amount_value, self.amount_mode).map_err(|error| {
            LifecycleError::InvalidContext {
                field: "amount",
                detail: error.to_string(),
            }
        })?;
        let selected_payments = self
            .selected_payment_methods
            .iter()
            .map(|method| {
                PaymentMethod::new(method).map_err(|error| LifecycleError::InvalidContext {
                    field: "selectedPaymentMethods",
                    detail: error.to_string(),
                })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let filters = EligibilityFilters::new(EligibilityFiltersInput {
            selected_payments,
            payment_logic: self.payment_logic,
            minimum_orders: self.minimum_orders,
            minimum_completion_percent: decimal(
                "minimumCompletionPercent",
                &self.minimum_completion_percent,
            )?,
            minimum_positive_percent: decimal(
                "minimumPositivePercent",
                &self.minimum_positive_percent,
            )?,
            pro_only: self.pro_only,
            maximum_buy_price: optional_decimal("maximumBuyPrice", &self.maximum_buy_price)?,
            minimum_sell_price: optional_decimal("minimumSellPrice", &self.minimum_sell_price)?,
        })
        .map_err(|error| LifecycleError::InvalidContext {
            field: "filters",
            detail: error.to_string(),
        })?;
        let target = ResultsTarget::new(self.results_target).map_err(|error| {
            LifecycleError::InvalidContext {
                field: "resultsTarget",
                detail: error.to_string(),
            }
        })?;
        Ok(ValidatedContext {
            pair,
            amount,
            filters,
            target,
        })
    }
}

fn decimal(field: &'static str, value: &str) -> Result<ExactDecimal, LifecycleError> {
    ExactDecimal::from_str(value).map_err(|error| LifecycleError::InvalidContext {
        field,
        detail: error.to_string(),
    })
}

fn optional_decimal(
    field: &'static str,
    value: &Option<String>,
) -> Result<Option<ExactDecimal>, LifecycleError> {
    value
        .as_deref()
        .map(|value| decimal(field, value))
        .transpose()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedContext {
    pub pair: MarketPair,
    pub amount: RequestedAmount,
    pub filters: EligibilityFilters,
    pub target: ResultsTarget,
}

impl ValidatedContext {
    pub fn persistence_context(&self) -> SnapshotContext {
        SnapshotContext {
            pair: self.pair.clone(),
            amount: self.amount,
            filters: self.filters.clone(),
            result_target: self.target,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartupStage {
    LoadingSettings,
    RestoringContext,
    LoadingCatalog,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefreshTrigger {
    Apply,
    Manual,
    Automatic,
    Startup,
    Wake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefreshStage {
    Queued,
    Acquiring,
    Validating,
    Calculating,
    Committing,
    Maintaining,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmptyKind {
    ProviderEmpty,
    NoMatchingResults,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureKind {
    InvalidRestoredState,
    Offline,
    Provider,
    Validation,
    Calculation,
    Persistence,
    Cancelled,
    Busy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionableFailure {
    pub kind: FailureKind,
    pub title: String,
    pub detail: String,
    pub retryable: bool,
    pub action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LifecycleStatus {
    Loading {
        stage: StartupStage,
    },
    Ready {
        last_success_ms: Option<i64>,
    },
    Refreshing {
        request_id: String,
        trigger: RefreshTrigger,
        stage: RefreshStage,
        previous_values_hidden: bool,
    },
    Empty {
        empty_kind: EmptyKind,
        detail: String,
        retryable: bool,
    },
    Error {
        failure: ActionableFailure,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Freshness {
    NeverLoaded,
    Fresh,
    Stale,
    ClockAnomaly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleView {
    pub status: LifecycleStatus,
    pub settings: RefreshSettings,
    pub draft: MarketContextDraft,
    pub applied: MarketContextDraft,
    pub unapplied_changes: bool,
    pub last_success_ms: Option<i64>,
    pub next_refresh_due_ms: Option<i64>,
    pub seconds_until_refresh: Option<u32>,
    pub freshness: Freshness,
    pub maintenance_warning: Option<String>,
    pub request_id: Option<String>,
    pub offline: bool,
}

#[derive(Clone, Debug)]
pub struct LifecycleController {
    status: LifecycleStatus,
    settings: RefreshSettings,
    draft: MarketContextDraft,
    applied: MarketContextDraft,
    last_success_ms: Option<i64>,
    last_attempt_ms: Option<i64>,
    last_request_id: Option<String>,
    maintenance_warning: Option<String>,
    offline: bool,
}

impl LifecycleController {
    pub fn loading() -> Self {
        let context = MarketContextDraft::first_run_default();
        Self {
            status: LifecycleStatus::Loading {
                stage: StartupStage::LoadingSettings,
            },
            settings: RefreshSettings::default(),
            draft: context.clone(),
            applied: context,
            last_success_ms: None,
            last_attempt_ms: None,
            last_request_id: None,
            maintenance_warning: None,
            offline: false,
        }
    }

    pub fn restore(&mut self, restored: PersistedLifecycle) -> Result<(), LifecycleError> {
        self.status = LifecycleStatus::Loading {
            stage: StartupStage::RestoringContext,
        };
        restored.validate().map_err(|error| {
            self.status = LifecycleStatus::Error {
                failure: ActionableFailure::invalid_restored(error.to_string()),
            };
            LifecycleError::InvalidRestoredState
        })?;
        self.settings = restored.settings;
        self.draft = restored.draft;
        self.applied = restored.applied;
        self.last_success_ms = restored.last_success_ms;
        self.last_attempt_ms = restored.last_success_ms;
        self.last_request_id = None;
        self.status = LifecycleStatus::Loading {
            stage: StartupStage::LoadingCatalog,
        };
        Ok(())
    }

    pub fn ready(&mut self) {
        self.status = LifecycleStatus::Ready {
            last_success_ms: self.last_success_ms,
        };
    }

    pub fn update_draft(&mut self, draft: MarketContextDraft) -> Result<(), LifecycleError> {
        self.ensure_restored_state_valid()?;
        draft.validate()?;
        self.draft = draft;
        Ok(())
    }

    pub fn apply_draft(&mut self) -> Result<(), LifecycleError> {
        self.ensure_restored_state_valid()?;
        self.draft.validate()?;
        self.applied = self.draft.clone();
        Ok(())
    }

    pub fn update_settings(&mut self, settings: RefreshSettings) -> Result<(), LifecycleError> {
        self.ensure_restored_state_valid()?;
        self.settings = settings.validate()?;
        Ok(())
    }

    pub fn begin_refresh(
        &mut self,
        request_id: &StableId,
        trigger: RefreshTrigger,
        started_ms: i64,
    ) -> Result<(), LifecycleError> {
        self.ensure_restored_state_valid()?;
        if self.offline {
            return Err(LifecycleError::Offline);
        }
        if matches!(self.status, LifecycleStatus::Refreshing { .. }) {
            return Err(LifecycleError::RefreshInProgress);
        }
        if started_ms < 0 {
            return Err(LifecycleError::InvalidRefreshTimestamp);
        }
        self.last_attempt_ms = Some(started_ms);
        self.last_request_id = Some(request_id.as_str().to_owned());
        self.maintenance_warning = None;
        self.status = LifecycleStatus::Refreshing {
            request_id: request_id.as_str().to_owned(),
            trigger,
            stage: RefreshStage::Queued,
            previous_values_hidden: true,
        };
        Ok(())
    }

    pub fn advance(&mut self, stage: RefreshStage) -> Result<(), LifecycleError> {
        match &mut self.status {
            LifecycleStatus::Refreshing { stage: current, .. } => {
                *current = stage;
                Ok(())
            }
            _ => Err(LifecycleError::NoRefreshInProgress),
        }
    }

    pub fn finish_success(&mut self, committed_ms: i64) -> Result<(), LifecycleError> {
        if !matches!(self.status, LifecycleStatus::Refreshing { .. }) {
            return Err(LifecycleError::NoRefreshInProgress);
        }
        self.finish_committed_success(committed_ms);
        Ok(())
    }

    pub fn finish_committed_success(&mut self, committed_ms: i64) {
        self.last_success_ms = Some(committed_ms);
        self.status = if self.offline {
            LifecycleStatus::Error {
                failure: ActionableFailure::offline(),
            }
        } else {
            LifecycleStatus::Ready {
                last_success_ms: Some(committed_ms),
            }
        };
    }

    pub fn finish_empty(&mut self, kind: EmptyKind, detail: impl Into<String>) {
        self.status = LifecycleStatus::Empty {
            empty_kind: kind,
            detail: detail.into(),
            retryable: true,
        };
    }

    pub fn finish_error(&mut self, failure: ActionableFailure) {
        self.status = LifecycleStatus::Error { failure };
    }

    pub fn finish_cancelled(&mut self) {
        if self.offline {
            self.status = LifecycleStatus::Error {
                failure: ActionableFailure::offline(),
            };
            return;
        }
        self.status = LifecycleStatus::Error {
            failure: ActionableFailure {
                kind: FailureKind::Cancelled,
                title: "Refresh cancelled".to_owned(),
                detail: "No partial or previous values were published.".to_owned(),
                retryable: true,
                action: "Refresh when ready".to_owned(),
            },
        };
    }

    pub fn set_offline(&mut self, offline: bool) {
        let was_offline_failure = matches!(
            self.status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Offline,
                    ..
                }
            }
        );
        self.offline = offline;
        if offline && !matches!(self.status, LifecycleStatus::Refreshing { .. }) {
            self.status = LifecycleStatus::Error {
                failure: ActionableFailure::offline(),
            };
        } else if !offline && was_offline_failure {
            self.status = LifecycleStatus::Ready {
                last_success_ms: self.last_success_ms,
            };
        }
    }

    pub fn record_maintenance_warning(&mut self, detail: impl Into<String>) {
        let detail = detail.into();
        self.maintenance_warning = Some(match self.maintenance_warning.take() {
            Some(existing) => format!("{existing} {detail}"),
            None => detail,
        });
    }

    fn ensure_restored_state_valid(&self) -> Result<(), LifecycleError> {
        if matches!(
            self.status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::InvalidRestoredState,
                    ..
                }
            }
        ) {
            return Err(LifecycleError::InvalidRestoredState);
        }
        Ok(())
    }

    pub fn due(&self, now_ms: i64) -> bool {
        matches!(self.status, LifecycleStatus::Ready { .. }) && self.due_after_wake(now_ms)
    }

    pub fn due_after_wake(&self, now_ms: i64) -> bool {
        self.settings.auto_refresh
            && !self.offline
            && self.last_success_ms.is_none_or(|last| {
                now_ms < last || now_ms >= last.saturating_add(self.settings.interval_ms())
            })
    }

    pub fn retry_due(&self, now_ms: i64) -> bool {
        let retryable_state = matches!(self.status, LifecycleStatus::Empty { .. })
            || matches!(
                self.status,
                LifecycleStatus::Error {
                    failure: ActionableFailure {
                        kind: FailureKind::Provider,
                        ..
                    }
                }
            );
        self.settings.auto_refresh
            && !self.offline
            && retryable_state
            && self.last_attempt_ms.is_none_or(|last| {
                now_ms < last || now_ms >= last.saturating_add(self.settings.interval_ms())
            })
    }

    pub fn view(&self, now_ms: i64) -> LifecycleView {
        let freshness = freshness(self.last_success_ms, self.settings, now_ms);
        let status = match (&self.status, freshness) {
            (LifecycleStatus::Ready { .. }, Freshness::Stale) => LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Validation,
                    title: "Live values are stale".to_owned(),
                    detail: "The freshness deadline passed, so live values were removed."
                        .to_owned(),
                    retryable: true,
                    action: "Refresh now".to_owned(),
                },
            },
            (LifecycleStatus::Ready { .. }, Freshness::ClockAnomaly) => LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Validation,
                    title: "System clock changed".to_owned(),
                    detail: "Live values were removed until a fresh observation can be validated."
                        .to_owned(),
                    retryable: true,
                    action: "Refresh now".to_owned(),
                },
            },
            _ => self.status.clone(),
        };
        let next_refresh_due_ms = if self.settings.auto_refresh {
            self.last_success_ms
                .map(|last| last.saturating_add(self.settings.interval_ms()))
        } else {
            None
        };
        let seconds_until_refresh = next_refresh_due_ms.map(|due| {
            if due <= now_ms {
                0
            } else {
                u32::try_from((due - now_ms + 999) / 1_000).unwrap_or(u32::MAX)
            }
        });
        LifecycleView {
            status,
            settings: self.settings,
            draft: self.draft.clone(),
            applied: self.applied.clone(),
            unapplied_changes: self.draft != self.applied,
            last_success_ms: self.last_success_ms,
            next_refresh_due_ms,
            seconds_until_refresh,
            freshness,
            maintenance_warning: self.maintenance_warning.clone(),
            request_id: self.last_request_id.clone(),
            offline: self.offline,
        }
    }

    pub fn persisted(&self) -> PersistedLifecycle {
        PersistedLifecycle {
            version: PERSISTED_LIFECYCLE_VERSION,
            settings: self.settings,
            draft: self.draft.clone(),
            applied: self.applied.clone(),
            last_success_ms: self.last_success_ms,
        }
    }

    pub fn settings(&self) -> RefreshSettings {
        self.settings
    }

    pub fn draft(&self) -> &MarketContextDraft {
        &self.draft
    }

    pub fn applied(&self) -> &MarketContextDraft {
        &self.applied
    }
}

impl ActionableFailure {
    fn offline() -> Self {
        Self {
            kind: FailureKind::Offline,
            title: "You are offline".to_owned(),
            detail: "Live values were removed because the provider cannot be reached.".to_owned(),
            retryable: true,
            action: "Reconnect, then refresh".to_owned(),
        }
    }

    pub fn invalid_restored(detail: String) -> Self {
        Self {
            kind: FailureKind::InvalidRestoredState,
            title: "Saved settings could not be restored".to_owned(),
            detail,
            retryable: false,
            action: "Review or reset saved settings explicitly".to_owned(),
        }
    }
}

fn freshness(last_success_ms: Option<i64>, settings: RefreshSettings, now_ms: i64) -> Freshness {
    let Some(last) = last_success_ms else {
        return Freshness::NeverLoaded;
    };
    if now_ms < last {
        return Freshness::ClockAnomaly;
    }
    if now_ms.saturating_sub(last) > settings.stale_after_ms() {
        Freshness::Stale
    } else {
        Freshness::Fresh
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedAcquisition {
    Publish(Box<Acquisition>),
    Empty(EmptyKind),
}

pub fn prepare_acquisition_for_publication(
    acquisition: Acquisition,
    context: &ValidatedContext,
) -> Result<PreparedAcquisition, LifecycleError> {
    if acquisition.buy.quality.provider_total() == Some(0)
        && acquisition.sell.quality.provider_total() == Some(0)
    {
        return Ok(PreparedAcquisition::Empty(EmptyKind::ProviderEmpty));
    }
    let buy = prepare_side(acquisition.buy, UserIntent::BuyAsset, context)?;
    let sell = prepare_side(acquisition.sell, UserIntent::SellAsset, context)?;
    if buy.ads.is_empty() || sell.ads.is_empty() {
        return Ok(PreparedAcquisition::Empty(EmptyKind::NoMatchingResults));
    }
    if !buy.quality.complete() || !sell.quality.complete() {
        return Err(LifecycleError::InsufficientEligibleResults);
    }
    Ok(PreparedAcquisition::Publish(Box::new(Acquisition {
        request_id: acquisition.request_id,
        pair: acquisition.pair,
        buy,
        sell,
        page_receipts: acquisition.page_receipts,
    })))
}

fn prepare_side(
    side: SideAcquisition,
    intent: UserIntent,
    context: &ValidatedContext,
) -> Result<SideAcquisition, LifecycleError> {
    let by_id = side
        .ads
        .into_iter()
        .map(|ad| (ad.ad.stable_id().as_str().to_owned(), ad))
        .collect::<BTreeMap<_, _>>();
    let domain_ads = by_id.values().map(|ad| ad.ad.clone()).collect::<Vec<_>>();
    let offers = rank_offers(intent, context.amount, &context.filters, &domain_ads)
        .map_err(|error| LifecycleError::Calculation(error.to_string()))?;
    let mut eligible = Vec::new();
    for offer in offers {
        if offer.evaluation().eligible() && eligible.len() < usize::from(context.target.value()) {
            let id = offer.ad().stable_id().as_str();
            let normalized = by_id.get(id).ok_or_else(|| {
                LifecycleError::Calculation("ranked ad identity was lost".to_owned())
            })?;
            eligible.push(normalized.clone());
        }
    }
    let valid = u32::try_from(eligible.len()).map_err(|_| LifecycleError::Quality)?;
    let original_valid = side.quality.valid();
    let locally_rejected = original_valid.saturating_sub(valid);
    let quality = p2p_domain::SideQuality::new(
        side.quality.fetched(),
        valid,
        side.quality.duplicates(),
        side.quality.rejected().saturating_add(locally_rejected),
        u32::from(context.target.value()),
        side.quality.provider_total(),
        side.quality.exhausted(),
    )
    .map_err(|_| LifecycleError::Quality)?;
    Ok(SideAcquisition {
        ads: eligible,
        quality,
        rejection_counts: side.rejection_counts,
    })
}

pub fn validate_acquisition_pair(
    acquisition: &Acquisition,
    expected: &ValidatedContext,
) -> Result<(), LifecycleError> {
    if acquisition.pair != expected.pair {
        return Err(LifecycleError::AcquisitionContextMismatch);
    }
    Ok(())
}

pub fn validate_acquisition_context(
    acquisition: &Acquisition,
    expected: &ValidatedContext,
) -> Result<(), LifecycleError> {
    validate_acquisition_pair(acquisition, expected)?;
    for (intent, side) in [
        (UserIntent::BuyAsset, &acquisition.buy),
        (UserIntent::SellAsset, &acquisition.sell),
    ] {
        for normalized in &side.ads {
            let evaluation =
                evaluate_eligibility(intent, expected.amount, &expected.filters, &normalized.ad)
                    .map_err(|error| LifecycleError::Calculation(error.to_string()))?;
            if !evaluation.eligible() {
                return Err(LifecycleError::IneligiblePublication);
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LifecycleError {
    #[error("refresh interval {supplied} is outside {minimum}..={maximum} seconds")]
    InvalidRefreshInterval {
        minimum: u32,
        maximum: u32,
        supplied: u32,
    },
    #[error("invalid context field {field}: {detail}")]
    InvalidContext { field: &'static str, detail: String },
    #[error("persisted lifecycle version {0} is unsupported")]
    UnsupportedPersistedVersion(u32),
    #[error("persisted last-success timestamp is invalid")]
    InvalidLastSuccessTimestamp,
    #[error("saved settings or context are invalid and were not silently replaced")]
    InvalidRestoredState,
    #[error("refresh start timestamp is invalid")]
    InvalidRefreshTimestamp,
    #[error("a refresh is already in progress")]
    RefreshInProgress,
    #[error("no refresh is in progress")]
    NoRefreshInProgress,
    #[error("refresh is blocked while offline")]
    Offline,
    #[error("the acquisition does not match the applied market context")]
    AcquisitionContextMismatch,
    #[error("the acquisition contains an ineligible ad")]
    IneligiblePublication,
    #[error("not enough eligible results were acquired before a non-exhausted stop")]
    InsufficientEligibleResults,
    #[error("calculation failed: {0}")]
    Calculation(String),
    #[error("quality counters could not be represented")]
    Quality,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> StableId {
        StableId::new("synthetic-lifecycle-request").expect("id")
    }

    #[test]
    fn first_run_defaults_are_valid_and_auto_refresh_is_on_at_twenty_seconds() {
        assert_eq!(RefreshSettings::default().interval_seconds, 20);
        assert!(RefreshSettings::default().auto_refresh);
        MarketContextDraft::first_run_default()
            .validate()
            .expect("valid first-run context");
    }

    #[test]
    fn settings_range_is_integer_and_fail_closed() {
        for interval in [10, 20, 3_600] {
            RefreshSettings {
                auto_refresh: true,
                interval_seconds: interval,
            }
            .validate()
            .expect("in range");
        }
        for interval in [0, 9, 3_601, u32::MAX] {
            assert!(matches!(
                RefreshSettings {
                    auto_refresh: true,
                    interval_seconds: interval,
                }
                .validate(),
                Err(LifecycleError::InvalidRefreshInterval { .. })
            ));
        }
    }

    #[test]
    fn invalid_restored_context_is_visible_and_never_replaced_by_defaults() {
        let mut controller = LifecycleController::loading();
        let mut invalid = MarketContextDraft::first_run_default();
        invalid.amount = "not-a-number".to_owned();
        assert_eq!(
            controller.restore(PersistedLifecycle {
                draft: invalid.clone(),
                ..PersistedLifecycle::default()
            }),
            Err(LifecycleError::InvalidRestoredState)
        );
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::InvalidRestoredState,
                    ..
                }
            }
        ));
        assert_ne!(controller.draft(), &invalid);
        assert_eq!(
            controller.update_draft(MarketContextDraft::first_run_default()),
            Err(LifecycleError::InvalidRestoredState)
        );
        assert_eq!(
            controller.update_settings(RefreshSettings::default()),
            Err(LifecycleError::InvalidRestoredState)
        );
        assert_eq!(
            controller.apply_draft(),
            Err(LifecycleError::InvalidRestoredState)
        );
        assert_eq!(
            controller.begin_refresh(&request_id(), RefreshTrigger::Manual, 0),
            Err(LifecycleError::InvalidRestoredState)
        );

        let mut timestamp_controller = LifecycleController::loading();
        assert_eq!(
            timestamp_controller.restore(PersistedLifecycle {
                last_success_ms: Some(-1),
                ..PersistedLifecycle::default()
            }),
            Err(LifecycleError::InvalidRestoredState)
        );
    }

    #[test]
    fn draft_and_applied_are_shared_but_unapplied_changes_are_explicit() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        let mut changed = MarketContextDraft::first_run_default();
        changed.amount = "12000".to_owned();
        controller.update_draft(changed.clone()).expect("draft");
        assert!(controller.view(0).unapplied_changes);
        assert_ne!(controller.applied(), &changed);
        controller.apply_draft().expect("apply");
        assert!(!controller.view(0).unapplied_changes);
        assert_eq!(controller.applied(), &changed);
    }

    #[test]
    fn refresh_hides_previous_values_and_cannot_overlap() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        let view = controller.view(10);
        assert!(matches!(
            view.status,
            LifecycleStatus::Refreshing {
                previous_values_hidden: true,
                ..
            }
        ));
        let json = serde_json::to_value(view).expect("serialize lifecycle view");
        assert_eq!(json["status"]["kind"], "refreshing");
        assert_eq!(json["requestId"], "synthetic-lifecycle-request");
        assert_eq!(json["status"]["previousValuesHidden"], true);
        assert!(json["status"].get("previous_values_hidden").is_none());
        assert_eq!(
            controller.begin_refresh(&request_id(), RefreshTrigger::Automatic, 0),
            Err(LifecycleError::RefreshInProgress)
        );
    }

    #[test]
    fn countdown_is_success_relative_and_failures_do_not_reset_it() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        controller.finish_success(100_000).expect("success");
        assert_eq!(controller.view(105_000).seconds_until_refresh, Some(15));
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Automatic, 106_000)
            .expect("begin failure");
        controller.finish_error(ActionableFailure {
            kind: FailureKind::Provider,
            title: "Provider failed".to_owned(),
            detail: "synthetic".to_owned(),
            retryable: true,
            action: "Retry".to_owned(),
        });
        assert_eq!(controller.view(106_000).next_refresh_due_ms, Some(120_000));
        assert!(!controller.retry_due(125_999));
        assert!(controller.retry_due(126_000));
    }

    #[test]
    fn stale_deadline_and_clock_jump_are_explicit() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        controller.finish_success(100_000).expect("success");
        assert_eq!(controller.view(160_000).freshness, Freshness::Fresh);
        let stale = controller.view(160_001);
        assert_eq!(stale.freshness, Freshness::Stale);
        assert!(matches!(stale.status, LifecycleStatus::Error { .. }));
        let clock_anomaly = controller.view(99_999);
        assert_eq!(clock_anomaly.freshness, Freshness::ClockAnomaly);
        assert!(matches!(
            clock_anomaly.status,
            LifecycleStatus::Error { .. }
        ));
        assert!(controller.due(99_999));
    }

    #[test]
    fn offline_and_cancelled_states_never_expose_live_values() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        controller.set_offline(true);
        assert_eq!(
            controller.begin_refresh(&request_id(), RefreshTrigger::Manual, 0),
            Err(LifecycleError::Offline)
        );
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Offline,
                    ..
                }
            }
        ));
        controller.finish_cancelled();
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Offline,
                    ..
                }
            }
        ));
        controller.set_offline(false);
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        controller.set_offline(true);
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Refreshing { .. }
        ));
        controller.finish_cancelled();
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Offline,
                    ..
                }
            }
        ));
        controller.set_offline(false);
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        controller.finish_cancelled();
        assert!(matches!(
            controller.view(0).status,
            LifecycleStatus::Error {
                failure: ActionableFailure {
                    kind: FailureKind::Cancelled,
                    ..
                }
            }
        ));
    }

    #[test]
    fn pruning_warning_does_not_reclassify_committed_success() {
        let mut controller = LifecycleController::loading();
        controller.ready();
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Manual, 0)
            .expect("begin");
        controller.finish_success(10).expect("success");
        controller.record_maintenance_warning("synthetic prune failure");
        let view = controller.view(10);
        assert!(matches!(view.status, LifecycleStatus::Ready { .. }));
        assert_eq!(
            view.maintenance_warning.as_deref(),
            Some("synthetic prune failure")
        );
    }

    fn synthetic_ad(
        intent: UserIntent,
        index: u8,
        monthly_orders: u64,
    ) -> p2p_provider::NormalizedAd {
        use p2p_domain::{
            AdvertiserSide, MerchantFacts, PaymentMethod, ValidatedAd, ValidatedAdInput,
        };
        let advertiser_side = match intent {
            UserIntent::BuyAsset => AdvertiserSide::Sell,
            UserIntent::SellAsset => AdvertiserSide::Buy,
        };
        let merchant = MerchantFacts::new(
            StableId::new(format!("synthetic-merchant-{intent:?}-{index}")).expect("merchant ID"),
            monthly_orders,
            ExactDecimal::from_i64(99),
            ExactDecimal::from_i64(99),
            true,
        )
        .expect("merchant");
        let ad = ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new(format!("synthetic-ad-{intent:?}-{index}")).expect("ad ID"),
            advertiser_side,
            price: ExactDecimal::from_i64(50),
            min_fiat: ExactDecimal::from_i64(100),
            max_fiat: ExactDecimal::from_i64(100_000),
            available_asset: ExactDecimal::from_i64(10_000),
            payments: [PaymentMethod::new("SYNTHETIC_PAY").expect("payment")]
                .into_iter()
                .collect(),
            merchant,
            observed_at_ms: 1_000,
        })
        .expect("ad");
        p2p_provider::NormalizedAd {
            ad,
            public_nickname: None,
            merchant_active_seconds: 5,
        }
    }

    fn synthetic_acquisition(exhausted: bool, low_order_first: bool) -> Acquisition {
        use p2p_domain::{PageReceiptTiming, SideQuality};
        let make_side = |intent| {
            let ads = (0..20)
                .map(|index| {
                    synthetic_ad(
                        intent,
                        index,
                        if low_order_first && index == 0 {
                            0
                        } else {
                            100
                        },
                    )
                })
                .collect::<Vec<_>>();
            SideAcquisition {
                quality: SideQuality::new(20, 20, 0, 0, 20, Some(20), exhausted).expect("quality"),
                ads,
                rejection_counts: BTreeMap::new(),
            }
        };
        Acquisition {
            request_id: request_id(),
            pair: MarketContextDraft::first_run_default()
                .validate()
                .expect("context")
                .pair,
            buy: make_side(UserIntent::BuyAsset),
            sell: make_side(UserIntent::SellAsset),
            page_receipts: vec![
                PageReceiptTiming::new(UserIntent::BuyAsset, 1, 1_000).expect("receipt"),
                PageReceiptTiming::new(UserIntent::SellAsset, 1, 1_001).expect("receipt"),
            ],
        }
    }

    #[test]
    fn preparation_ranks_filters_and_keeps_only_eligible_two_side_results() {
        let mut draft = MarketContextDraft::first_run_default();
        draft.results_target = 20;
        let context = draft.validate().expect("context");
        let prepared =
            prepare_acquisition_for_publication(synthetic_acquisition(false, false), &context)
                .expect("prepare");
        let PreparedAcquisition::Publish(acquisition) = prepared else {
            panic!("expected publication");
        };
        assert_eq!(acquisition.buy.ads.len(), 20);
        assert_eq!(acquisition.sell.ads.len(), 20);
        validate_acquisition_context(&acquisition, &context).expect("publication context");
    }

    #[test]
    fn local_filter_shortfall_never_publishes_partial_results() {
        let mut draft = MarketContextDraft::first_run_default();
        draft.results_target = 20;
        draft.minimum_orders = 50;
        let context = draft.validate().expect("context");
        assert_eq!(
            prepare_acquisition_for_publication(synthetic_acquisition(false, true), &context,),
            Err(LifecycleError::InsufficientEligibleResults)
        );
    }

    #[test]
    fn provider_empty_and_filter_no_match_are_distinct_typed_states() {
        use p2p_domain::SideQuality;
        let mut draft = MarketContextDraft::first_run_default();
        draft.results_target = 20;
        let context = draft.validate().expect("context");
        let empty_side = || SideAcquisition {
            ads: Vec::new(),
            quality: SideQuality::new(0, 0, 0, 0, 20, Some(0), true).expect("quality"),
            rejection_counts: BTreeMap::new(),
        };
        let provider_empty = Acquisition {
            request_id: request_id(),
            pair: context.pair.clone(),
            buy: empty_side(),
            sell: empty_side(),
            page_receipts: Vec::new(),
        };
        assert_eq!(
            prepare_acquisition_for_publication(provider_empty, &context),
            Ok(PreparedAcquisition::Empty(EmptyKind::ProviderEmpty))
        );

        draft.minimum_orders = 200;
        let filtered_context = draft.validate().expect("filtered context");
        assert_eq!(
            prepare_acquisition_for_publication(
                synthetic_acquisition(true, false),
                &filtered_context,
            ),
            Ok(PreparedAcquisition::Empty(EmptyKind::NoMatchingResults))
        );

        let mut controller = LifecycleController::loading();
        controller.ready();
        controller
            .begin_refresh(&request_id(), RefreshTrigger::Automatic, 100)
            .expect("begin empty refresh");
        controller.finish_empty(EmptyKind::ProviderEmpty, "synthetic empty");
        assert!(!controller.retry_due(20_099));
        assert!(controller.retry_due(20_100));
    }
}
