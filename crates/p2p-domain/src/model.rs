use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::{AdvertiserSide, AmountMode, ExactDecimal, PaymentLogic, UserIntent};

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DomainValidationError {
    #[error("asset and fiat symbols must be canonical uppercase ASCII symbols")]
    InvalidSymbol,
    #[error("asset and fiat symbols must differ")]
    SamePairSymbol,
    #[error("stable identifiers must be non-empty printable ASCII without whitespace")]
    InvalidStableId,
    #[error("payment method identifiers must be non-empty printable ASCII without whitespace")]
    InvalidPaymentMethod,
    #[error("a validated advertisement must expose at least one payment method")]
    MissingPaymentMethods,
    #[error("result target must be an integer between 20 and 1000")]
    InvalidResultsTarget,
    #[error("amount must be greater than zero")]
    InvalidAmount,
    #[error("price must be greater than zero")]
    InvalidPrice,
    #[error("limits and availability must be non-negative and internally ordered")]
    InvalidRange,
    #[error("percentage must be between zero and one hundred inclusive")]
    InvalidPercentage,
    #[error("source timestamps are inconsistent")]
    InvalidTimestamps,
    #[error("page receipt timing requires a one-based page number")]
    InvalidPageNumber,
    #[error("page receipts must cover both sides in contiguous per-side order")]
    InvalidPageSequence,
    #[error("quality counters are inconsistent")]
    InvalidQualityCounters,
    #[error("auto-refresh interval must be between 10 and 3600 seconds")]
    InvalidRefreshInterval,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Symbol(String);

impl Symbol {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        let valid = (2..=20).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
        if !valid {
            return Err(DomainValidationError::InvalidSymbol);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for Symbol {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Symbol {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketPair {
    asset: Symbol,
    fiat: Symbol,
}

impl MarketPair {
    pub fn new(asset: Symbol, fiat: Symbol) -> Result<Self, DomainValidationError> {
        if asset == fiat {
            return Err(DomainValidationError::SamePairSymbol);
        }
        Ok(Self { asset, fiat })
    }

    pub fn asset(&self) -> &Symbol {
        &self.asset
    }

    pub fn fiat(&self) -> &Symbol {
        &self.fiat
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StableId(String);

impl StableId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        let valid = (1..=128).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b',' | b'"'));
        if !valid {
            return Err(DomainValidationError::InvalidStableId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for StableId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PaymentMethod(String);

impl PaymentMethod {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainValidationError> {
        let value = value.into();
        let valid = (1..=64).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b',' | b'"'));
        if !valid {
            return Err(DomainValidationError::InvalidPaymentMethod);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for PaymentMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PaymentMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ResultsTarget(u16);

impl ResultsTarget {
    pub const MINIMUM: u16 = 20;
    pub const MAXIMUM: u16 = 1000;

    pub fn new(value: u16) -> Result<Self, DomainValidationError> {
        if !(Self::MINIMUM..=Self::MAXIMUM).contains(&value) {
            return Err(DomainValidationError::InvalidResultsTarget);
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedAmount {
    value: ExactDecimal,
    mode: AmountMode,
}

impl RequestedAmount {
    pub fn new(value: ExactDecimal, mode: AmountMode) -> Result<Self, DomainValidationError> {
        if !value.is_positive() {
            return Err(DomainValidationError::InvalidAmount);
        }
        Ok(Self { value, mode })
    }

    pub fn value(self) -> ExactDecimal {
        self.value
    }

    pub fn mode(self) -> AmountMode {
        self.mode
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantFacts {
    stable_id: StableId,
    monthly_orders: u64,
    completion_percent: ExactDecimal,
    positive_percent: ExactDecimal,
    is_pro: bool,
}

impl MerchantFacts {
    pub fn new(
        stable_id: StableId,
        monthly_orders: u64,
        completion_percent: ExactDecimal,
        positive_percent: ExactDecimal,
        is_pro: bool,
    ) -> Result<Self, DomainValidationError> {
        validate_percentage(completion_percent)?;
        validate_percentage(positive_percent)?;
        Ok(Self {
            stable_id,
            monthly_orders,
            completion_percent,
            positive_percent,
            is_pro,
        })
    }

    pub fn stable_id(&self) -> &StableId {
        &self.stable_id
    }

    pub fn monthly_orders(&self) -> u64 {
        self.monthly_orders
    }

    pub fn completion_percent(&self) -> ExactDecimal {
        self.completion_percent
    }

    pub fn positive_percent(&self) -> ExactDecimal {
        self.positive_percent
    }

    pub fn is_pro(&self) -> bool {
        self.is_pro
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedAd {
    stable_id: StableId,
    advertiser_side: AdvertiserSide,
    price: ExactDecimal,
    min_fiat: ExactDecimal,
    max_fiat: ExactDecimal,
    available_asset: ExactDecimal,
    payments: BTreeSet<PaymentMethod>,
    merchant: MerchantFacts,
    observed_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedAdInput {
    pub stable_id: StableId,
    pub advertiser_side: AdvertiserSide,
    pub price: ExactDecimal,
    pub min_fiat: ExactDecimal,
    pub max_fiat: ExactDecimal,
    pub available_asset: ExactDecimal,
    pub payments: BTreeSet<PaymentMethod>,
    pub merchant: MerchantFacts,
    pub observed_at_ms: i64,
}

impl ValidatedAd {
    pub fn new(input: ValidatedAdInput) -> Result<Self, DomainValidationError> {
        if !input.price.is_positive() {
            return Err(DomainValidationError::InvalidPrice);
        }
        if input.min_fiat.is_negative()
            || input.max_fiat.is_negative()
            || input.available_asset.is_negative()
            || input.max_fiat < input.min_fiat
        {
            return Err(DomainValidationError::InvalidRange);
        }
        if input.payments.is_empty() {
            return Err(DomainValidationError::MissingPaymentMethods);
        }
        Ok(Self {
            stable_id: input.stable_id,
            advertiser_side: input.advertiser_side,
            price: input.price,
            min_fiat: input.min_fiat,
            max_fiat: input.max_fiat,
            available_asset: input.available_asset,
            payments: input.payments,
            merchant: input.merchant,
            observed_at_ms: input.observed_at_ms,
        })
    }

    pub fn stable_id(&self) -> &StableId {
        &self.stable_id
    }

    pub fn advertiser_side(&self) -> AdvertiserSide {
        self.advertiser_side
    }

    pub fn price(&self) -> ExactDecimal {
        self.price
    }

    pub fn min_fiat(&self) -> ExactDecimal {
        self.min_fiat
    }

    pub fn max_fiat(&self) -> ExactDecimal {
        self.max_fiat
    }

    pub fn available_asset(&self) -> ExactDecimal {
        self.available_asset
    }

    pub fn payments(&self) -> &BTreeSet<PaymentMethod> {
        &self.payments
    }

    pub fn merchant(&self) -> &MerchantFacts {
        &self.merchant
    }

    pub fn observed_at_ms(&self) -> i64 {
        self.observed_at_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityFilters {
    selected_payments: BTreeSet<PaymentMethod>,
    payment_logic: PaymentLogic,
    minimum_orders: u64,
    minimum_completion_percent: ExactDecimal,
    minimum_positive_percent: ExactDecimal,
    pro_only: bool,
    maximum_buy_price: Option<ExactDecimal>,
    minimum_sell_price: Option<ExactDecimal>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EligibilityFiltersInput {
    pub selected_payments: BTreeSet<PaymentMethod>,
    pub payment_logic: PaymentLogic,
    pub minimum_orders: u64,
    pub minimum_completion_percent: ExactDecimal,
    pub minimum_positive_percent: ExactDecimal,
    pub pro_only: bool,
    pub maximum_buy_price: Option<ExactDecimal>,
    pub minimum_sell_price: Option<ExactDecimal>,
}

impl EligibilityFilters {
    pub fn new(input: EligibilityFiltersInput) -> Result<Self, DomainValidationError> {
        validate_percentage(input.minimum_completion_percent)?;
        validate_percentage(input.minimum_positive_percent)?;
        if input
            .maximum_buy_price
            .is_some_and(|value| !value.is_positive())
            || input
                .minimum_sell_price
                .is_some_and(|value| !value.is_positive())
        {
            return Err(DomainValidationError::InvalidPrice);
        }
        Ok(Self {
            selected_payments: input.selected_payments,
            payment_logic: input.payment_logic,
            minimum_orders: input.minimum_orders,
            minimum_completion_percent: input.minimum_completion_percent,
            minimum_positive_percent: input.minimum_positive_percent,
            pro_only: input.pro_only,
            maximum_buy_price: input.maximum_buy_price,
            minimum_sell_price: input.minimum_sell_price,
        })
    }

    pub fn neutral() -> Self {
        Self {
            selected_payments: BTreeSet::new(),
            payment_logic: PaymentLogic::Any,
            minimum_orders: 0,
            minimum_completion_percent: ExactDecimal::ZERO,
            minimum_positive_percent: ExactDecimal::ZERO,
            pro_only: false,
            maximum_buy_price: None,
            minimum_sell_price: None,
        }
    }

    pub fn selected_payments(&self) -> &BTreeSet<PaymentMethod> {
        &self.selected_payments
    }

    pub fn payment_logic(&self) -> PaymentLogic {
        self.payment_logic
    }

    pub fn minimum_orders(&self) -> u64 {
        self.minimum_orders
    }

    pub fn minimum_completion_percent(&self) -> ExactDecimal {
        self.minimum_completion_percent
    }

    pub fn minimum_positive_percent(&self) -> ExactDecimal {
        self.minimum_positive_percent
    }

    pub fn pro_only(&self) -> bool {
        self.pro_only
    }

    pub fn maximum_buy_price(&self) -> Option<ExactDecimal> {
        self.maximum_buy_price
    }

    pub fn minimum_sell_price(&self) -> Option<ExactDecimal> {
        self.minimum_sell_price
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationTimestamps {
    request_started_ms: i64,
    last_page_received_ms: i64,
    validated_ms: i64,
    committed_ms: i64,
    agent_checked_ms: Option<i64>,
}

impl ObservationTimestamps {
    pub fn new(
        request_started_ms: i64,
        last_page_received_ms: i64,
        validated_ms: i64,
        committed_ms: i64,
        agent_checked_ms: Option<i64>,
    ) -> Result<Self, DomainValidationError> {
        if request_started_ms > last_page_received_ms
            || last_page_received_ms > validated_ms
            || validated_ms > committed_ms
        {
            return Err(DomainValidationError::InvalidTimestamps);
        }
        Ok(Self {
            request_started_ms,
            last_page_received_ms,
            validated_ms,
            committed_ms,
            agent_checked_ms,
        })
    }

    pub fn request_started_ms(self) -> i64 {
        self.request_started_ms
    }

    pub fn last_page_received_ms(self) -> i64 {
        self.last_page_received_ms
    }

    pub fn validated_ms(self) -> i64 {
        self.validated_ms
    }

    pub fn committed_ms(self) -> i64 {
        self.committed_ms
    }

    pub fn agent_checked_ms(self) -> Option<i64> {
        self.agent_checked_ms
    }

    pub fn age_ms(self, now_ms: i64) -> Option<u64> {
        now_ms
            .checked_sub(self.committed_ms)
            .and_then(|age| u64::try_from(age).ok())
    }

    pub fn freshness(
        self,
        now_ms: i64,
        refresh_interval_seconds: u32,
    ) -> Result<Freshness, DomainValidationError> {
        if !(10..=3600).contains(&refresh_interval_seconds) {
            return Err(DomainValidationError::InvalidRefreshInterval);
        }
        let Some(age_ms) = self.age_ms(now_ms) else {
            return Ok(Freshness::ClockInvalid);
        };
        let deadline_ms = u64::from(refresh_interval_seconds)
            .saturating_mul(2_000)
            .max(60_000);
        Ok(if age_ms <= deadline_ms {
            Freshness::Fresh {
                age_ms,
                deadline_ms,
            }
        } else {
            Freshness::Stale {
                age_ms,
                deadline_ms,
            }
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Freshness {
    Fresh { age_ms: u64, deadline_ms: u64 },
    Stale { age_ms: u64, deadline_ms: u64 },
    ClockInvalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideQuality {
    fetched: u32,
    valid: u32,
    duplicates: u32,
    rejected: u32,
    target: u32,
    provider_total: Option<u32>,
    exhausted: bool,
}

impl SideQuality {
    pub fn new(
        fetched: u32,
        valid: u32,
        duplicates: u32,
        rejected: u32,
        target: u32,
        provider_total: Option<u32>,
        exhausted: bool,
    ) -> Result<Self, DomainValidationError> {
        if !(u32::from(ResultsTarget::MINIMUM)..=u32::from(ResultsTarget::MAXIMUM))
            .contains(&target)
            || valid.saturating_add(duplicates).saturating_add(rejected) > fetched
            || provider_total.is_some_and(|total| fetched > total)
        {
            return Err(DomainValidationError::InvalidQualityCounters);
        }
        Ok(Self {
            fetched,
            valid,
            duplicates,
            rejected,
            target,
            provider_total,
            exhausted,
        })
    }

    pub fn fetched(self) -> u32 {
        self.fetched
    }

    pub fn valid(self) -> u32 {
        self.valid
    }

    pub fn duplicates(self) -> u32 {
        self.duplicates
    }

    pub fn rejected(self) -> u32 {
        self.rejected
    }

    pub fn target(self) -> u32 {
        self.target
    }

    pub fn provider_total(self) -> Option<u32> {
        self.provider_total
    }

    pub fn exhausted(self) -> bool {
        self.exhausted
    }

    pub fn complete(self) -> bool {
        self.valid >= self.target || self.exhausted
    }

    pub fn provider_coverage(self) -> Result<Option<ExactDecimal>, crate::ArithmeticError> {
        match self.provider_total {
            Some(0) | None => Ok(None),
            Some(total) => Ok(Some(
                ExactDecimal::from_u64(u64::from(self.valid))
                    .checked_div(ExactDecimal::from_u64(u64::from(total)))?,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    ExperimentalBinanceP2pWeb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReceiptTiming {
    intent: UserIntent,
    page: u16,
    received_ms: i64,
}

impl PageReceiptTiming {
    pub fn new(
        intent: UserIntent,
        page: u16,
        received_ms: i64,
    ) -> Result<Self, DomainValidationError> {
        if page == 0 {
            return Err(DomainValidationError::InvalidPageNumber);
        }
        Ok(Self {
            intent,
            page,
            received_ms,
        })
    }

    pub fn intent(self) -> UserIntent {
        self.intent
    }

    pub fn page(self) -> u16 {
        self.page
    }

    pub fn received_ms(self) -> i64 {
        self.received_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotProvenance {
    source: SourceKind,
    adapter_version: StableId,
    request_id: StableId,
    timestamps: ObservationTimestamps,
    page_receipts: Vec<PageReceiptTiming>,
    buy_quality: SideQuality,
    sell_quality: SideQuality,
}

impl SnapshotProvenance {
    pub fn new(
        adapter_version: StableId,
        request_id: StableId,
        timestamps: ObservationTimestamps,
        page_receipts: Vec<PageReceiptTiming>,
        buy_quality: SideQuality,
        sell_quality: SideQuality,
    ) -> Result<Self, DomainValidationError> {
        if page_receipts.iter().any(|receipt| {
            receipt.received_ms < timestamps.request_started_ms
                || receipt.received_ms > timestamps.last_page_received_ms
        }) || page_receipts
            .windows(2)
            .any(|pair| pair[0].received_ms > pair[1].received_ms)
        {
            return Err(DomainValidationError::InvalidTimestamps);
        }
        for intent in [UserIntent::BuyAsset, UserIntent::SellAsset] {
            let pages = page_receipts
                .iter()
                .filter(|receipt| receipt.intent == intent)
                .map(|receipt| receipt.page)
                .collect::<Vec<_>>();
            let valid_sequence = !pages.is_empty()
                && pages.iter().enumerate().all(|(index, page)| {
                    u16::try_from(index + 1).is_ok_and(|expected| *page == expected)
                });
            if !valid_sequence {
                return Err(DomainValidationError::InvalidPageSequence);
            }
        }
        Ok(Self {
            source: SourceKind::ExperimentalBinanceP2pWeb,
            adapter_version,
            request_id,
            timestamps,
            page_receipts,
            buy_quality,
            sell_quality,
        })
    }

    pub fn source(&self) -> SourceKind {
        self.source
    }

    pub fn adapter_version(&self) -> &StableId {
        &self.adapter_version
    }

    pub fn request_id(&self) -> &StableId {
        &self.request_id
    }

    pub fn timestamps(&self) -> ObservationTimestamps {
        self.timestamps
    }

    pub fn page_receipts(&self) -> &[PageReceiptTiming] {
        &self.page_receipts
    }

    pub fn buy_quality(&self) -> SideQuality {
        self.buy_quality
    }

    pub fn sell_quality(&self) -> SideQuality {
        self.sell_quality
    }

    pub fn publishable(
        &self,
        now_ms: i64,
        refresh_interval_seconds: u32,
    ) -> Result<bool, DomainValidationError> {
        Ok(self.buy_quality.complete()
            && self.sell_quality.complete()
            && matches!(
                self.timestamps
                    .freshness(now_ms, refresh_interval_seconds)?,
                Freshness::Fresh { .. }
            ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DomainFailureCategory {
    InvalidInput,
    WrongSide,
    IncompatibleRoute,
    InsufficientCoverage,
    Stale,
    Arithmetic,
    UnknownCosts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainFailure {
    pub code: StableId,
    pub category: DomainFailureCategory,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculationContext {
    pub pair: MarketPair,
    pub intent: UserIntent,
    pub amount: RequestedAmount,
    pub filters: EligibilityFilters,
    pub result_target: ResultsTarget,
}

fn validate_percentage(value: ExactDecimal) -> Result<(), DomainValidationError> {
    if value.is_negative() || value > ExactDecimal::HUNDRED {
        return Err(DomainValidationError::InvalidPercentage);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_symbols_are_canonical_and_distinct() {
        let usdt = Symbol::new("USDT").expect("canonical");
        let egp = Symbol::new("EGP").expect("canonical");
        assert!(MarketPair::new(usdt.clone(), egp).is_ok());
        assert_eq!(
            MarketPair::new(usdt.clone(), usdt),
            Err(DomainValidationError::SamePairSymbol)
        );
        assert!(Symbol::new("usdt").is_err());
    }

    #[test]
    fn stale_deadline_is_the_greater_of_sixty_seconds_and_twice_interval() {
        let times = ObservationTimestamps::new(0, 1, 2, 3, None).expect("ordered");
        assert_eq!(
            times.freshness(60_003, 20).expect("valid interval"),
            Freshness::Fresh {
                age_ms: 60_000,
                deadline_ms: 60_000,
            }
        );
        assert_eq!(
            times.freshness(60_004, 20).expect("valid interval"),
            Freshness::Stale {
                age_ms: 60_001,
                deadline_ms: 60_000,
            }
        );
        assert_eq!(
            times.freshness(200_003, 100).expect("valid interval"),
            Freshness::Fresh {
                age_ms: 200_000,
                deadline_ms: 200_000,
            }
        );
    }

    #[test]
    fn future_commit_time_is_clock_invalid_not_fresh() {
        let times = ObservationTimestamps::new(10, 11, 12, 13, None).expect("ordered");
        assert_eq!(
            times.freshness(12, 20).expect("valid interval"),
            Freshness::ClockInvalid
        );
    }

    #[test]
    fn result_target_enforces_the_approved_inclusive_bounds() {
        assert!(ResultsTarget::new(19).is_err());
        assert_eq!(ResultsTarget::new(20).expect("minimum").value(), 20);
        assert_eq!(ResultsTarget::new(1000).expect("maximum").value(), 1000);
        assert!(ResultsTarget::new(1001).is_err());
    }

    #[test]
    fn quality_and_provenance_fail_closed_on_inconsistent_counts_or_staleness() {
        assert!(SideQuality::new(10, 11, 0, 0, 20, Some(10), true).is_err());
        let quality =
            SideQuality::new(20, 20, 0, 0, 20, Some(40), false).expect("consistent quality");
        assert!(quality.complete());
        assert_eq!(
            quality
                .provider_coverage()
                .expect("coverage")
                .expect("known total")
                .canonical(),
            "0.5"
        );
        let timestamps = ObservationTimestamps::new(0, 10, 11, 12, None).expect("timestamps");
        let provenance = SnapshotProvenance::new(
            StableId::new("adapter-1").expect("id"),
            StableId::new("request-1").expect("id"),
            timestamps,
            vec![
                PageReceiptTiming::new(UserIntent::BuyAsset, 1, 5).expect("receipt"),
                PageReceiptTiming::new(UserIntent::SellAsset, 1, 10).expect("receipt"),
            ],
            quality,
            quality,
        )
        .expect("provenance");
        assert_eq!(provenance.source(), SourceKind::ExperimentalBinanceP2pWeb);
        assert!(provenance.publishable(60_012, 20).expect("fresh"));
        assert!(!provenance.publishable(60_013, 20).expect("stale"));
    }
}
