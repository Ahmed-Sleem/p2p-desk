use sha2::{Digest, Sha256};

pub const DATABASE_SCHEMA_VERSION: u32 = 1;
pub const DATABASE_FILE_NAME: &str = "p2p-desk.sqlite3";
pub const IDENTITY_KEY_FILE_NAME: &str = "identity.key";
pub const BUSY_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_MANAGED_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_DETAIL_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
pub const DEFAULT_SUMMARY_RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1_000;
pub const DEFAULT_ROLLUP_RETENTION_MS: i64 = 2 * 365 * 24 * 60 * 60 * 1_000;
pub const AUTOMATIC_BACKUP_LIMIT: usize = 5;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATION_1_SQL: &str = r#"
CREATE TABLE schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    checksum_sha256 TEXT NOT NULL CHECK(length(checksum_sha256) = 64),
    applied_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE metadata (
    key TEXT PRIMARY KEY CHECK(length(key) BETWEEN 1 AND 80),
    value TEXT NOT NULL CHECK(length(value) <= 4096),
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE contexts (
    context_hash TEXT PRIMARY KEY CHECK(length(context_hash) = 64),
    asset TEXT NOT NULL CHECK(length(asset) BETWEEN 2 AND 20),
    fiat TEXT NOT NULL CHECK(length(fiat) BETWEEN 2 AND 20),
    amount_text TEXT NOT NULL CHECK(length(amount_text) BETWEEN 1 AND 80),
    amount_mode TEXT NOT NULL CHECK(amount_mode IN ('fiat', 'asset')),
    selected_payments_json TEXT NOT NULL CHECK(length(selected_payments_json) <= 65536),
    payment_logic TEXT NOT NULL CHECK(payment_logic IN ('ANY', 'ALL')),
    minimum_orders INTEGER NOT NULL CHECK(minimum_orders >= 0),
    minimum_completion_percent_text TEXT NOT NULL CHECK(length(minimum_completion_percent_text) BETWEEN 1 AND 80),
    minimum_positive_percent_text TEXT NOT NULL CHECK(length(minimum_positive_percent_text) BETWEEN 1 AND 80),
    pro_only INTEGER NOT NULL CHECK(pro_only IN (0, 1)),
    maximum_buy_price_text TEXT CHECK(maximum_buy_price_text IS NULL OR length(maximum_buy_price_text) BETWEEN 1 AND 80),
    minimum_sell_price_text TEXT CHECK(minimum_sell_price_text IS NULL OR length(minimum_sell_price_text) BETWEEN 1 AND 80),
    result_target INTEGER NOT NULL CHECK(result_target BETWEEN 20 AND 1000)
) STRICT;

CREATE TABLE snapshots (
    snapshot_id TEXT PRIMARY KEY CHECK(length(snapshot_id) = 34),
    request_key TEXT NOT NULL UNIQUE CHECK(length(request_key) = 64),
    context_hash TEXT NOT NULL REFERENCES contexts(context_hash) ON DELETE RESTRICT,
    source_kind TEXT NOT NULL CHECK(source_kind = 'experimental-binance-p2p-web'),
    provider_adapter_version TEXT NOT NULL CHECK(length(provider_adapter_version) BETWEEN 1 AND 128),
    domain_schema_version TEXT NOT NULL CHECK(length(domain_schema_version) BETWEEN 1 AND 128),
    calculation_version TEXT NOT NULL CHECK(length(calculation_version) BETWEEN 1 AND 128),
    app_version TEXT NOT NULL CHECK(length(app_version) BETWEEN 1 AND 128),
    request_started_ms INTEGER NOT NULL,
    last_page_received_ms INTEGER NOT NULL,
    validated_ms INTEGER NOT NULL,
    committed_ms INTEGER NOT NULL,
    agent_checked_ms INTEGER,
    buy_fetched INTEGER NOT NULL CHECK(buy_fetched >= 0),
    buy_valid INTEGER NOT NULL CHECK(buy_valid >= 0),
    buy_duplicates INTEGER NOT NULL CHECK(buy_duplicates >= 0),
    buy_rejected INTEGER NOT NULL CHECK(buy_rejected >= 0),
    buy_target INTEGER NOT NULL CHECK(buy_target BETWEEN 20 AND 1000),
    buy_provider_total INTEGER CHECK(buy_provider_total IS NULL OR buy_provider_total >= 0),
    buy_exhausted INTEGER NOT NULL CHECK(buy_exhausted IN (0, 1)),
    sell_fetched INTEGER NOT NULL CHECK(sell_fetched >= 0),
    sell_valid INTEGER NOT NULL CHECK(sell_valid >= 0),
    sell_duplicates INTEGER NOT NULL CHECK(sell_duplicates >= 0),
    sell_rejected INTEGER NOT NULL CHECK(sell_rejected >= 0),
    sell_target INTEGER NOT NULL CHECK(sell_target BETWEEN 20 AND 1000),
    sell_provider_total INTEGER CHECK(sell_provider_total IS NULL OR sell_provider_total >= 0),
    sell_exhausted INTEGER NOT NULL CHECK(sell_exhausted IN (0, 1)),
    buy_rejection_counts_json TEXT NOT NULL CHECK(length(buy_rejection_counts_json) <= 65536),
    sell_rejection_counts_json TEXT NOT NULL CHECK(length(sell_rejection_counts_json) <= 65536),
    completion_state TEXT NOT NULL CHECK(completion_state = 'complete'),
    CHECK(request_started_ms <= last_page_received_ms),
    CHECK(last_page_received_ms <= validated_ms),
    CHECK(validated_ms <= committed_ms)
) STRICT;

CREATE INDEX snapshots_committed_idx ON snapshots(committed_ms);
CREATE INDEX snapshots_context_idx ON snapshots(context_hash, committed_ms);

CREATE TABLE snapshot_pages (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(snapshot_id) ON DELETE CASCADE,
    user_intent TEXT NOT NULL CHECK(user_intent IN ('buy-asset', 'sell-asset')),
    page_number INTEGER NOT NULL CHECK(page_number BETWEEN 1 AND 50),
    received_at_ms INTEGER NOT NULL,
    PRIMARY KEY(snapshot_id, user_intent, page_number)
) STRICT, WITHOUT ROWID;

CREATE TABLE ad_versions (
    content_hash TEXT PRIMARY KEY CHECK(length(content_hash) = 64),
    ad_key TEXT NOT NULL CHECK(length(ad_key) = 64),
    merchant_key TEXT NOT NULL CHECK(length(merchant_key) = 64),
    advertiser_side TEXT NOT NULL CHECK(advertiser_side IN ('BUY', 'SELL')),
    price_text TEXT NOT NULL CHECK(length(price_text) BETWEEN 1 AND 80),
    min_fiat_text TEXT NOT NULL CHECK(length(min_fiat_text) BETWEEN 1 AND 80),
    max_fiat_text TEXT NOT NULL CHECK(length(max_fiat_text) BETWEEN 1 AND 80),
    available_asset_text TEXT NOT NULL CHECK(length(available_asset_text) BETWEEN 1 AND 80),
    monthly_orders INTEGER NOT NULL CHECK(monthly_orders >= 0),
    completion_percent_text TEXT NOT NULL CHECK(length(completion_percent_text) BETWEEN 1 AND 80),
    positive_percent_text TEXT NOT NULL CHECK(length(positive_percent_text) BETWEEN 1 AND 80),
    is_pro INTEGER NOT NULL CHECK(is_pro IN (0, 1)),
    merchant_active_seconds INTEGER NOT NULL CHECK(merchant_active_seconds >= 0),
    first_stored_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX ad_versions_ad_key_idx ON ad_versions(ad_key);
CREATE INDEX ad_versions_merchant_key_idx ON ad_versions(merchant_key);

CREATE TABLE ad_version_payments (
    content_hash TEXT NOT NULL REFERENCES ad_versions(content_hash) ON DELETE CASCADE,
    payment_method TEXT NOT NULL CHECK(length(payment_method) BETWEEN 1 AND 64),
    PRIMARY KEY(content_hash, payment_method)
) STRICT, WITHOUT ROWID;

CREATE TABLE snapshot_ad_membership (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(snapshot_id) ON DELETE CASCADE,
    user_intent TEXT NOT NULL CHECK(user_intent IN ('buy-asset', 'sell-asset')),
    rank_position INTEGER NOT NULL CHECK(rank_position >= 0),
    content_hash TEXT NOT NULL REFERENCES ad_versions(content_hash) ON DELETE RESTRICT,
    observed_at_ms INTEGER NOT NULL,
    PRIMARY KEY(snapshot_id, user_intent, rank_position),
    UNIQUE(snapshot_id, user_intent, content_hash)
) STRICT, WITHOUT ROWID;

CREATE INDEX snapshot_membership_content_idx ON snapshot_ad_membership(content_hash);

CREATE TABLE snapshot_summaries (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(snapshot_id) ON DELETE CASCADE,
    user_intent TEXT NOT NULL CHECK(user_intent IN ('buy-asset', 'sell-asset')),
    metric_key TEXT NOT NULL CHECK(length(metric_key) BETWEEN 1 AND 80),
    value_text TEXT CHECK(value_text IS NULL OR length(value_text) BETWEEN 1 AND 80),
    unit TEXT NOT NULL CHECK(length(unit) BETWEEN 1 AND 40),
    PRIMARY KEY(snapshot_id, user_intent, metric_key)
) STRICT, WITHOUT ROWID;

CREATE TABLE history_rollups (
    rollup_id TEXT PRIMARY KEY CHECK(length(rollup_id) = 34),
    asset TEXT NOT NULL CHECK(length(asset) BETWEEN 2 AND 20),
    fiat TEXT NOT NULL CHECK(length(fiat) BETWEEN 2 AND 20),
    user_intent TEXT NOT NULL CHECK(user_intent IN ('buy-asset', 'sell-asset')),
    period_kind TEXT NOT NULL CHECK(period_kind IN ('hour', 'day')),
    period_start_ms INTEGER NOT NULL,
    metric_key TEXT NOT NULL CHECK(length(metric_key) BETWEEN 1 AND 80),
    value_text TEXT CHECK(value_text IS NULL OR length(value_text) BETWEEN 1 AND 80),
    unit TEXT NOT NULL CHECK(length(unit) BETWEEN 1 AND 40),
    sample_count INTEGER NOT NULL CHECK(sample_count >= 0),
    calculation_version TEXT NOT NULL CHECK(length(calculation_version) BETWEEN 1 AND 128),
    UNIQUE(asset, fiat, user_intent, period_kind, period_start_ms, metric_key, calculation_version)
) STRICT;

CREATE INDEX history_rollups_period_idx ON history_rollups(period_start_ms);

CREATE TABLE pair_catalog (
    pair_key TEXT PRIMARY KEY CHECK(length(pair_key) BETWEEN 5 AND 41),
    asset TEXT NOT NULL CHECK(length(asset) BETWEEN 2 AND 20),
    fiat TEXT NOT NULL CHECK(length(fiat) BETWEEN 2 AND 20),
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
    disabled_reason TEXT CHECK(disabled_reason IS NULL OR length(disabled_reason) BETWEEN 1 AND 500),
    verified_at_ms INTEGER NOT NULL,
    disabled_at_ms INTEGER,
    provider_adapter_version TEXT NOT NULL CHECK(length(provider_adapter_version) BETWEEN 1 AND 128),
    precision_json TEXT NOT NULL CHECK(length(precision_json) <= 65536),
    UNIQUE(asset, fiat),
    CHECK(
        (enabled = 1 AND disabled_reason IS NULL AND disabled_at_ms IS NULL)
        OR (enabled = 0 AND disabled_reason IS NOT NULL AND disabled_at_ms IS NOT NULL)
    )
) STRICT;

CREATE TABLE pair_catalog_payments (
    pair_key TEXT NOT NULL REFERENCES pair_catalog(pair_key) ON DELETE CASCADE,
    payment_method TEXT NOT NULL CHECK(length(payment_method) BETWEEN 1 AND 64),
    PRIMARY KEY(pair_key, payment_method)
) STRICT, WITHOUT ROWID;

CREATE TABLE cost_profiles (
    profile_id TEXT PRIMARY KEY CHECK(length(profile_id) = 34),
    asset TEXT NOT NULL CHECK(length(asset) BETWEEN 2 AND 20),
    fiat TEXT NOT NULL CHECK(length(fiat) BETWEEN 2 AND 20),
    route_key TEXT NOT NULL CHECK(length(route_key) BETWEEN 1 AND 200),
    leg TEXT NOT NULL CHECK(leg IN ('buy-asset', 'sell-asset')),
    payment_method TEXT NOT NULL CHECK(length(payment_method) BETWEEN 1 AND 64),
    created_at_ms INTEGER NOT NULL,
    UNIQUE(asset, fiat, route_key, leg, payment_method)
) STRICT;

CREATE TABLE cost_profile_versions (
    version_id TEXT PRIMARY KEY CHECK(length(version_id) = 64),
    profile_id TEXT NOT NULL REFERENCES cost_profiles(profile_id) ON DELETE RESTRICT,
    effective_from_ms INTEGER NOT NULL,
    effective_to_ms INTEGER,
    label TEXT NOT NULL CHECK(length(label) BETWEEN 1 AND 120),
    fixed_fiat_text TEXT CHECK(fixed_fiat_text IS NULL OR length(fixed_fiat_text) BETWEEN 1 AND 80),
    percent_fiat_text TEXT CHECK(percent_fiat_text IS NULL OR length(percent_fiat_text) BETWEEN 1 AND 80),
    fixed_asset_text TEXT CHECK(fixed_asset_text IS NULL OR length(fixed_asset_text) BETWEEN 1 AND 80),
    minimum_charge_text TEXT CHECK(minimum_charge_text IS NULL OR length(minimum_charge_text) BETWEEN 1 AND 80),
    maximum_charge_text TEXT CHECK(maximum_charge_text IS NULL OR length(maximum_charge_text) BETWEEN 1 AND 80),
    fixed_buffer_text TEXT CHECK(fixed_buffer_text IS NULL OR length(fixed_buffer_text) BETWEEN 1 AND 80),
    percent_buffer_text TEXT CHECK(percent_buffer_text IS NULL OR length(percent_buffer_text) BETWEEN 1 AND 80),
    source_label TEXT CHECK(source_label IS NULL OR length(source_label) <= 200),
    note TEXT CHECK(note IS NULL OR length(note) <= 2000),
    created_at_ms INTEGER NOT NULL,
    CHECK(effective_to_ms IS NULL OR effective_to_ms > effective_from_ms)
) STRICT;

CREATE INDEX cost_versions_effective_idx ON cost_profile_versions(profile_id, effective_from_ms);

CREATE TABLE settings (
    section_key TEXT NOT NULL CHECK(length(section_key) BETWEEN 1 AND 80),
    setting_key TEXT NOT NULL CHECK(length(setting_key) BETWEEN 1 AND 80),
    value_json TEXT NOT NULL CHECK(length(value_json) <= 262144),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(section_key, setting_key)
) STRICT, WITHOUT ROWID;

CREATE TABLE chart_annotations (
    annotation_id TEXT PRIMARY KEY CHECK(length(annotation_id) = 34),
    chart_key TEXT NOT NULL CHECK(length(chart_key) BETWEEN 1 AND 120),
    context_hash TEXT CHECK(context_hash IS NULL OR length(context_hash) = 64),
    payload_json TEXT NOT NULL CHECK(length(payload_json) <= 1048576),
    schema_version INTEGER NOT NULL CHECK(schema_version >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE named_views (
    view_id TEXT PRIMARY KEY CHECK(length(view_id) = 34),
    chart_key TEXT NOT NULL CHECK(length(chart_key) BETWEEN 1 AND 120),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 120),
    context_hash TEXT CHECK(context_hash IS NULL OR length(context_hash) = 64),
    payload_json TEXT NOT NULL CHECK(length(payload_json) <= 1048576),
    schema_version INTEGER NOT NULL CHECK(schema_version >= 1),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE(chart_key, name, context_hash)
) STRICT;

CREATE TABLE report_audit (
    report_id TEXT PRIMARY KEY CHECK(length(report_id) = 34),
    snapshot_id TEXT NOT NULL CHECK(length(snapshot_id) = 34),
    generated_at_ms INTEGER NOT NULL,
    package_sha256 TEXT NOT NULL CHECK(length(package_sha256) = 64),
    destination_hint TEXT CHECK(destination_hint IS NULL OR length(destination_hint) <= 300)
) STRICT;

CREATE TABLE diagnostic_index (
    diagnostic_id TEXT PRIMARY KEY CHECK(length(diagnostic_id) = 34),
    occurred_at_ms INTEGER NOT NULL,
    level TEXT NOT NULL CHECK(level IN ('info', 'warning', 'error')),
    category TEXT NOT NULL CHECK(length(category) BETWEEN 1 AND 80),
    code TEXT NOT NULL CHECK(length(code) BETWEEN 1 AND 80),
    correlation_key TEXT CHECK(correlation_key IS NULL OR length(correlation_key) = 64)
) STRICT;

CREATE TABLE retention_events (
    event_id TEXT PRIMARY KEY CHECK(length(event_id) = 34),
    occurred_at_ms INTEGER NOT NULL,
    reason TEXT NOT NULL CHECK(reason IN ('expired-detail', 'expired-summary', 'expired-rollup', 'managed-cap')),
    snapshots_affected INTEGER NOT NULL CHECK(snapshots_affected >= 0),
    rows_deleted INTEGER NOT NULL CHECK(rows_deleted >= 0),
    managed_bytes_before INTEGER NOT NULL CHECK(managed_bytes_before >= 0),
    managed_bytes_after INTEGER NOT NULL CHECK(managed_bytes_after >= 0)
) STRICT;
"#;

pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial_atomic_persistence",
    sql: MIGRATION_1_SQL,
}];

pub fn migration_checksum(migration: &Migration) -> String {
    let mut digest = Sha256::new();
    digest.update(migration.sql.as_bytes());
    hex_lower(&digest.finalize())
}

pub fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
