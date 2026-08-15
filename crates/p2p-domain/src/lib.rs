mod costs;
mod decimal;
mod eligibility;
mod error;
mod market;
mod model;
mod multi_ad;
mod quote;
mod sensitivity;
mod statistics;
mod version;

pub use costs::{
    CompatibleSpread, CostBreakdown, CostInput, CostProfile, LegCostTerms, LegCostTermsInput,
    NetAvailability, NetSpread, compatible_spread,
};
pub use decimal::{ArithmeticError, DecimalParseError, ExactDecimal};
pub use eligibility::{EligibilityEvaluation, EligibilityReason, evaluate_eligibility};
pub use error::CalculationError;
pub use market::{AdvertiserSide, AmountMode, PaymentLogic, RequestSide, UserIntent};
pub use model::{
    CalculationContext, DomainFailure, DomainFailureCategory, DomainValidationError,
    EligibilityFilters, EligibilityFiltersInput, Freshness, MarketPair, MerchantFacts,
    ObservationTimestamps, PageReceiptTiming, PaymentMethod, RequestedAmount, ResultsTarget,
    SideQuality, SnapshotProvenance, SourceKind, StableId, Symbol, ValidatedAd, ValidatedAdInput,
};
pub use multi_ad::{
    AllocationCandidate, AllocationLeg, AllocationMethod, AllocationOutcome, OptimalityCertificate,
    allocate_across_ads, allocation_candidate_from_ad, allocation_frontier,
};
pub use quote::{QuoteFlow, RankedOffer, SingleAdQuote, rank_offers, single_ad_quote};
pub use sensitivity::{SensitivityPoint, amount_sensitivity};
pub use statistics::{
    DescriptiveSummary, OutlierClassification, OutlierStatus, StabilitySummary, WeightedValue,
    arithmetic_mean, descriptive_summary, herfindahl_hirschman_index,
    inverse_weighted_ecdf_quantile, jaccard_index, modified_z_outliers, r7_quantile,
    stability_summary, top_k_share, weighted_mean,
};
pub use version::{
    CALCULATION_VERSION, DOMAIN_SCHEMA_VERSION, FORMULA_METADATA, FormulaMetadata,
    MULTI_AD_CERTIFICATE_POLICY, MULTI_AD_OBJECTIVE, OUTLIER_COEFFICIENT_TEXT,
    OUTLIER_MINIMUM_SAMPLE, OUTLIER_THRESHOLD_TEXT, ROUNDING_POLICY, UNWEIGHTED_QUANTILE_POLICY,
    WEIGHTED_QUANTILE_POLICY,
};
