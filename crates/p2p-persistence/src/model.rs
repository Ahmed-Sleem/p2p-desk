use std::collections::BTreeSet;

use p2p_domain::{
    CALCULATION_VERSION, DOMAIN_SCHEMA_VERSION, EligibilityFilters, ExactDecimal, MarketPair,
    PaymentMethod, RequestedAmount, ResultsTarget, UserIntent,
};
use p2p_provider::{ADAPTER_VERSION, Acquisition};
use serde::{Deserialize, Serialize};

use crate::error::{PersistenceError, Result};
use crate::schema::{
    DATABASE_SCHEMA_VERSION, DEFAULT_DETAIL_RETENTION_MS, DEFAULT_MANAGED_CAP_BYTES,
    DEFAULT_ROLLUP_RETENTION_MS, DEFAULT_SUMMARY_RETENTION_MS,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeVersions {
    pub app_version: String,
    pub provider_adapter_version: String,
    pub domain_schema_version: String,
    pub calculation_version: String,
}

impl RuntimeVersions {
    pub fn current(app_version: impl Into<String>) -> Result<Self> {
        let versions = Self {
            app_version: app_version.into(),
            provider_adapter_version: ADAPTER_VERSION.to_owned(),
            domain_schema_version: DOMAIN_SCHEMA_VERSION.to_owned(),
            calculation_version: CALCULATION_VERSION.to_owned(),
        };
        versions.validate()?;
        Ok(versions)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("app version", self.app_version.as_str()),
            (
                "provider adapter version",
                self.provider_adapter_version.as_str(),
            ),
            ("domain schema version", self.domain_schema_version.as_str()),
            ("calculation version", self.calculation_version.as_str()),
        ] {
            if value.is_empty()
                || value.len() > 128
                || value.chars().any(|character| character.is_control())
            {
                return Err(PersistenceError::InvalidInput(format!(
                    "{name} must be 1–128 printable characters"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SnapshotContext {
    pub pair: MarketPair,
    pub amount: RequestedAmount,
    pub filters: EligibilityFilters,
    pub result_target: ResultsTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SummaryInput {
    pub intent: UserIntent,
    pub metric_key: String,
    pub value: Option<ExactDecimal>,
    pub unit: String,
}

#[derive(Clone, Debug)]
pub struct PublicationInput<'a> {
    pub acquisition: &'a Acquisition,
    pub context: SnapshotContext,
    pub request_started_ms: i64,
    pub last_page_received_ms: i64,
    pub validated_ms: i64,
    pub committed_ms: i64,
    pub agent_checked_ms: Option<i64>,
    pub refresh_interval_seconds: u32,
    pub summaries: Vec<SummaryInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationOutcome {
    pub snapshot_id: String,
    pub context_hash: String,
    pub inserted_ad_versions: u64,
    pub reused_ad_versions: u64,
    pub buy_memberships: u64,
    pub sell_memberships: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RollupPeriod {
    Hour,
    Day,
}

impl RollupPeriod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RollupInput {
    pub pair: MarketPair,
    pub intent: UserIntent,
    pub period: RollupPeriod,
    pub period_start_ms: i64,
    pub metric_key: String,
    pub value: Option<ExactDecimal>,
    pub unit: String,
    pub sample_count: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicy {
    pub detail_retention_ms: i64,
    pub summary_retention_ms: i64,
    pub rollup_retention_ms: i64,
    pub managed_cap_bytes: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            detail_retention_ms: DEFAULT_DETAIL_RETENTION_MS,
            summary_retention_ms: DEFAULT_SUMMARY_RETENTION_MS,
            rollup_retention_ms: DEFAULT_ROLLUP_RETENTION_MS,
            managed_cap_bytes: DEFAULT_MANAGED_CAP_BYTES,
        }
    }
}

impl RetentionPolicy {
    pub const MINIMUM_CAP_BYTES: u64 = 64 * 1024 * 1024;
    pub const MAXIMUM_CAP_BYTES: u64 = 16 * 1024 * 1024 * 1024;

    pub fn new(
        detail_retention_ms: i64,
        summary_retention_ms: i64,
        rollup_retention_ms: i64,
        managed_cap_bytes: u64,
    ) -> Result<Self> {
        let policy = Self {
            detail_retention_ms,
            summary_retention_ms,
            rollup_retention_ms,
            managed_cap_bytes,
        };
        if detail_retention_ms <= 0
            || summary_retention_ms < detail_retention_ms
            || rollup_retention_ms < summary_retention_ms
            || !(Self::MINIMUM_CAP_BYTES..=Self::MAXIMUM_CAP_BYTES).contains(&managed_cap_bytes)
        {
            return Err(PersistenceError::InvalidInput(
                "retention tiers must be positive and ordered, and the cap must be 64 MiB–16 GiB"
                    .to_owned(),
            ));
        }
        Ok(policy)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        detail_retention_ms: i64,
        summary_retention_ms: i64,
        rollup_retention_ms: i64,
        managed_cap_bytes: u64,
    ) -> Self {
        Self {
            detail_retention_ms,
            summary_retention_ms,
            rollup_retention_ms,
            managed_cap_bytes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneOutcome {
    pub detail_snapshots_pruned: u64,
    pub summary_snapshots_pruned: u64,
    pub rollups_pruned: u64,
    pub orphan_ad_versions_pruned: u64,
    pub cap_detail_snapshots_pruned: u64,
    pub managed_bytes_before: u64,
    pub managed_bytes_after: u64,
    pub cap_satisfied: bool,
}

#[derive(Clone, Debug)]
pub struct CostProfileInput {
    pub pair: MarketPair,
    pub route_key: String,
    pub leg: UserIntent,
    pub payment_method: PaymentMethod,
    pub label: String,
    pub effective_from_ms: i64,
    pub effective_to_ms: Option<i64>,
    pub fixed_fiat: Option<ExactDecimal>,
    pub percent_fiat: Option<ExactDecimal>,
    pub fixed_asset: Option<ExactDecimal>,
    pub minimum_charge: Option<ExactDecimal>,
    pub maximum_charge: Option<ExactDecimal>,
    pub fixed_buffer: Option<ExactDecimal>,
    pub percent_buffer: Option<ExactDecimal>,
    pub source_label: Option<String>,
    pub note: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostVersionOutcome {
    pub profile_id: String,
    pub version_id: String,
}

#[derive(Clone, Debug)]
pub struct CatalogPairInput {
    pub pair: MarketPair,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub verified_at_ms: i64,
    pub disabled_at_ms: Option<i64>,
    pub precision: serde_json::Value,
    pub payment_methods: BTreeSet<PaymentMethod>,
}

#[derive(Clone, Debug)]
pub struct AnnotationInput {
    pub chart_key: String,
    pub context_hash: Option<String>,
    pub payload: serde_json::Value,
    pub schema_version: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct NamedViewInput {
    pub chart_key: String,
    pub name: String,
    pub context_hash: Option<String>,
    pub payload: serde_json::Value,
    pub schema_version: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClearScope {
    History,
    AnnotationsAndViews,
    Settings,
    Logs,
    AllLocalData,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearOutcome {
    pub scope: ClearScope,
    pub database_rows_deleted: u64,
    pub filesystem_entries_deleted: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupEntry {
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format: String,
    pub format_version: u32,
    pub product: String,
    pub created_at_ms: i64,
    pub database_schema_version: u32,
    pub versions: RuntimeVersions,
    pub included_domains: Vec<String>,
    pub entries: Vec<BackupEntry>,
}

impl BackupManifest {
    pub const FORMAT: &'static str = "p2p-desk-local-backup";
    pub const FORMAT_VERSION: u32 = 1;
    const INCLUDED_DOMAINS: [&'static str; 7] = [
        "database",
        "settings",
        "pair-catalog",
        "cost-profiles",
        "annotations",
        "named-views",
        "pseudonymous-identity-key",
    ];

    pub(crate) fn new(
        created_at_ms: i64,
        database_schema_version: u32,
        versions: RuntimeVersions,
        entries: Vec<BackupEntry>,
    ) -> Self {
        Self {
            format: Self::FORMAT.to_owned(),
            format_version: Self::FORMAT_VERSION,
            product: "P2P Desk".to_owned(),
            created_at_ms,
            database_schema_version,
            versions,
            included_domains: Self::INCLUDED_DOMAINS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            entries,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.format != Self::FORMAT
            || self.format_version != Self::FORMAT_VERSION
            || self.product != "P2P Desk"
            || self.database_schema_version > DATABASE_SCHEMA_VERSION
        {
            return Err(PersistenceError::Incompatible(
                "backup format, product, or schema version is unsupported".to_owned(),
            ));
        }
        if self.created_at_ms < 0
            || self.included_domains.len() != Self::INCLUDED_DOMAINS.len()
            || !self
                .included_domains
                .iter()
                .map(String::as_str)
                .eq(Self::INCLUDED_DOMAINS)
        {
            return Err(PersistenceError::InvalidBackup(
                "backup timestamp or included-domain inventory is invalid".to_owned(),
            ));
        }
        if self.entries.iter().any(|entry| {
            entry.name.is_empty()
                || entry.size_bytes == 0
                || entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            return Err(PersistenceError::InvalidBackup(
                "backup entry metadata is invalid".to_owned(),
            ));
        }
        self.versions.validate().map_err(|error| {
            PersistenceError::InvalidBackup(format!("backup version metadata is invalid: {error}"))
        })?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOutcome {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub manifest: BackupManifest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub restored_schema_version: u32,
    pub migrated_to_schema_version: u32,
    pub safety_backup_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAdVersion {
    pub content_hash: String,
    pub ad_key: String,
    pub merchant_key: String,
    pub user_intent: UserIntent,
    pub rank_position: u32,
    pub advertiser_side: String,
    pub price: ExactDecimal,
    pub min_fiat: ExactDecimal,
    pub max_fiat: ExactDecimal,
    pub available_asset: ExactDecimal,
    pub payment_methods: Vec<String>,
    pub monthly_orders: u64,
    pub completion_percent: ExactDecimal,
    pub positive_percent: ExactDecimal,
    pub is_pro: bool,
    pub merchant_active_seconds: u64,
    pub observed_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSnapshot {
    pub snapshot_id: String,
    pub asset: String,
    pub fiat: String,
    pub amount: ExactDecimal,
    pub amount_mode: String,
    pub committed_ms: i64,
    pub provider_adapter_version: String,
    pub domain_schema_version: String,
    pub calculation_version: String,
    pub ads: Vec<StoredAdVersion>,
}
