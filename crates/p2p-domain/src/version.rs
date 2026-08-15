use serde::Serialize;

pub const CALCULATION_VERSION: &str = "p2p-desk-calc-1.0.1";
pub const DOMAIN_SCHEMA_VERSION: &str = "p2p-desk-domain-1.1.0";
pub const ROUNDING_POLICY: &str = "midpoint-nearest-even-at-explicit-output-boundaries";
pub const UNWEIGHTED_QUANTILE_POLICY: &str = "hyndman-fan-r7-linear";
pub const WEIGHTED_QUANTILE_POLICY: &str = "inverse-weighted-empirical-cdf";
pub const OUTLIER_COEFFICIENT_TEXT: &str = "0.6745";
pub const OUTLIER_THRESHOLD_TEXT: &str = "3.5";
pub const OUTLIER_MINIMUM_SAMPLE: usize = 5;
pub const MULTI_AD_OBJECTIVE: &str =
    "maximize-input-fill-then-output-then-fewer-legs-then-stable-ids";
pub const MULTI_AD_CERTIFICATE_POLICY: &str =
    "optimal-only-for-continuous-zero-minimum-zero-fixed-cost-sorted-greedy";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaMetadata {
    pub calculation_version: &'static str,
    pub domain_schema_version: &'static str,
    pub exact_decimal: &'static str,
    pub rounding: &'static str,
    pub unweighted_quantile: &'static str,
    pub weighted_quantile: &'static str,
    pub modified_z_formula: &'static str,
    pub modified_z_threshold: &'static str,
    pub modified_z_minimum_sample: usize,
    pub zero_mad_policy: &'static str,
    pub eligibility_comparison: &'static str,
    pub gross_spread_formula: &'static str,
    pub cost_formula: &'static str,
    pub unknown_cost_policy: &'static str,
    pub multi_ad_objective: &'static str,
    pub multi_ad_certificate: &'static str,
}

pub const FORMULA_METADATA: FormulaMetadata = FormulaMetadata {
    calculation_version: CALCULATION_VERSION,
    domain_schema_version: DOMAIN_SCHEMA_VERSION,
    exact_decimal: "fastnum-d256-checked-no-binary-float",
    rounding: ROUNDING_POLICY,
    unweighted_quantile: UNWEIGHTED_QUANTILE_POLICY,
    weighted_quantile: WEIGHTED_QUANTILE_POLICY,
    modified_z_formula: "0.6745-times-x-minus-median-divided-by-mad",
    modified_z_threshold: OUTLIER_THRESHOLD_TEXT,
    modified_z_minimum_sample: OUTLIER_MINIMUM_SAMPLE,
    zero_mad_policy: "indeterminate",
    eligibility_comparison: "exact-unrounded-amount-limit-availability-payment-merchant-price",
    gross_spread_formula: "sell-fiat-minus-buy-fiat-divided-by-buy-fiat",
    cost_formula: "clamp-fixed-fiat-plus-percent-gross-fiat-plus-fixed-asset-times-price-then-add-fixed-and-percent-buffer",
    unknown_cost_policy: "suppress-net-zero-must-be-explicit",
    multi_ad_objective: MULTI_AD_OBJECTIVE,
    multi_ad_certificate: MULTI_AD_CERTIFICATE_POLICY,
};
