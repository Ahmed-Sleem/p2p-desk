use std::fs::{self, OpenOptions as FileOpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use p2p_domain::{
    AmountMode, ExactDecimal, ObservationTimestamps, PaymentLogic, SnapshotProvenance, StableId,
    UserIntent,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::backup::{
    automatic_backup_path, create_backup_archive, ensure_supported_schema,
    retain_latest_automatic_backups, schema_version, validate_and_extract_backup,
};
use crate::error::{PersistenceError, Result};
use crate::hash::{
    ad_matches_intent, context_hash, cost_version_hash, intent_text, normalized_ad_record,
    pseudonym, random_identifier,
};
use crate::model::{
    AnnotationInput, BackupOutcome, CatalogPairInput, ClearOutcome, ClearScope, CostProfileInput,
    CostVersionOutcome, NamedViewInput, PruneOutcome, PublicationInput, PublicationOutcome,
    RestoreOutcome, RetentionPolicy, RollupInput, RuntimeVersions, StoredAdVersion, StoredSnapshot,
};
use crate::schema::{
    BUSY_TIMEOUT_MS, DATABASE_FILE_NAME, DATABASE_SCHEMA_VERSION, IDENTITY_KEY_FILE_NAME,
    MIGRATIONS, Migration, migration_checksum,
};

const DATABASE_DIRECTORY: &str = "database";
const AUTOMATIC_BACKUP_DIRECTORY: &str = "migration-backups";
const TEMPORARY_DIRECTORY: &str = "temporary";
const LOG_DIRECTORY: &str = "logs";
const CRASH_DIRECTORY: &str = "crash";
const RESTORE_MARKER_FILE_NAME: &str = "restore-in-progress";
const IDENTITY_KEY_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultPoint {
    None,
    MigrationBeforeCommit,
    PublicationAfterHeader,
    PublicationAfterAds,
    RestoreAfterDatabaseMoved,
    RestoreAfterKeyMoved,
    RestoreAfterDatabaseInstalled,
    RestoreAfterReplacement,
}

#[derive(Clone, Copy)]
struct StoreOptions {
    busy_timeout_ms: u64,
    fault: FaultPoint,
    available_space_override: Option<u64>,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            busy_timeout_ms: BUSY_TIMEOUT_MS,
            fault: FaultPoint::None,
            available_space_override: None,
        }
    }
}

struct StoreInner {
    connection: Option<Connection>,
    identity_key: [u8; IDENTITY_KEY_BYTES],
}

pub struct PersistenceStore {
    root: PathBuf,
    database_path: PathBuf,
    identity_key_path: PathBuf,
    automatic_backup_directory: PathBuf,
    temporary_directory: PathBuf,
    versions: RuntimeVersions,
    options: StoreOptions,
    inner: Mutex<StoreInner>,
}

impl PersistenceStore {
    pub fn open(
        root: impl AsRef<Path>,
        versions: RuntimeVersions,
        opened_at_ms: i64,
    ) -> Result<Self> {
        Self::open_with_options(
            root.as_ref(),
            versions,
            opened_at_ms,
            StoreOptions::default(),
        )
    }

    fn open_with_options(
        root: &Path,
        versions: RuntimeVersions,
        opened_at_ms: i64,
        options: StoreOptions,
    ) -> Result<Self> {
        versions.validate()?;
        create_data_directories(root)?;
        let database_path = root.join(DATABASE_DIRECTORY).join(DATABASE_FILE_NAME);
        let identity_key_path = root.join(DATABASE_DIRECTORY).join(IDENTITY_KEY_FILE_NAME);
        let automatic_backup_directory = root.join(AUTOMATIC_BACKUP_DIRECTORY);
        let temporary_directory = root.join(TEMPORARY_DIRECTORY);
        recover_interrupted_restore(&database_path, &identity_key_path)?;
        let existing_database = database_path
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 0);
        let identity_key = load_or_create_identity_key(&identity_key_path, existing_database)?;
        let mut connection = open_connection(&database_path, options.busy_timeout_ms)?;
        check_integrity(&connection, false)?;
        migrate_database(
            &mut connection,
            &identity_key_path,
            &automatic_backup_directory,
            &versions,
            opened_at_ms,
            existing_database,
            options.fault,
        )?;
        verify_migration_catalog(&connection)?;
        validate_compiled_schema(&connection)?;
        check_integrity(&connection, false)?;
        validate_semantic_storage(&connection)?;
        apply_max_page_count(&connection, RetentionPolicy::default().managed_cap_bytes)?;
        write_runtime_metadata(&connection, &versions, opened_at_ms)?;
        retain_latest_automatic_backups(&automatic_backup_directory)?;

        Ok(Self {
            root: root.to_path_buf(),
            database_path,
            identity_key_path,
            automatic_backup_directory,
            temporary_directory,
            versions,
            options,
            inner: Mutex::new(StoreInner {
                connection: Some(connection),
                identity_key,
            }),
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn schema_version(&self) -> Result<u32> {
        let guard = self.lock_inner()?;
        schema_version(connection_ref(&guard)?)
    }

    pub fn integrity_check(&self) -> Result<()> {
        let guard = self.lock_inner()?;
        check_integrity(connection_ref(&guard)?, true)
    }

    pub fn publish_complete_snapshot(
        &self,
        input: PublicationInput<'_>,
    ) -> Result<PublicationOutcome> {
        self.publish_with_fault(input, self.options.fault)
    }

    fn publish_with_fault(
        &self,
        input: PublicationInput<'_>,
        fault: FaultPoint,
    ) -> Result<PublicationOutcome> {
        validate_publication(&input)?;
        let mut guard = self.lock_inner()?;
        let identity_key = guard.identity_key;
        let connection = connection_mut(&mut guard)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;

        let context_key = insert_context(&transaction, &input)?;
        let snapshot_id = random_identifier("sn")?;
        let request_key = pseudonym(
            &identity_key,
            "request",
            input.acquisition.request_id.as_str(),
        );
        insert_snapshot_header(
            &transaction,
            &snapshot_id,
            &request_key,
            &context_key,
            &input,
            &self.versions,
        )?;
        if fault == FaultPoint::PublicationAfterHeader {
            return Err(PersistenceError::FaultInjected);
        }

        for receipt in &input.acquisition.page_receipts {
            transaction
                .execute(
                    "INSERT INTO snapshot_pages(snapshot_id, user_intent, page_number, received_at_ms) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        snapshot_id,
                        intent_text(receipt.intent()),
                        i64::from(receipt.page()),
                        receipt.received_ms()
                    ],
                )
                .map_err(PersistenceError::from)?;
        }

        let mut inserted_ad_versions = 0_u64;
        let mut reused_ad_versions = 0_u64;
        for (intent, ads) in [
            (UserIntent::BuyAsset, input.acquisition.buy.ads.as_slice()),
            (UserIntent::SellAsset, input.acquisition.sell.ads.as_slice()),
        ] {
            for (rank, normalized) in ads.iter().enumerate() {
                let record = normalized_ad_record(normalized, &identity_key);
                let inserted = transaction
                    .execute(
                        "INSERT OR IGNORE INTO ad_versions(
                            content_hash, ad_key, merchant_key, advertiser_side, price_text,
                            min_fiat_text, max_fiat_text, available_asset_text, monthly_orders,
                            completion_percent_text, positive_percent_text, is_pro,
                            merchant_active_seconds, first_stored_at_ms
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                        params![
                            record.content_hash,
                            record.ad_key,
                            record.merchant_key,
                            record.advertiser_side,
                            record.price_text,
                            record.min_fiat_text,
                            record.max_fiat_text,
                            record.available_asset_text,
                            u64_to_i64(record.monthly_orders, "monthly orders")?,
                            record.completion_percent_text,
                            record.positive_percent_text,
                            bool_i64(record.is_pro),
                            u64_to_i64(record.merchant_active_seconds, "merchant active seconds")?,
                            input.committed_ms,
                        ],
                    )
                    .map_err(PersistenceError::from)?;
                if inserted == 1 {
                    inserted_ad_versions = inserted_ad_versions.saturating_add(1);
                    for payment in &record.payment_methods {
                        transaction
                            .execute(
                                "INSERT INTO ad_version_payments(content_hash, payment_method) VALUES (?1, ?2)",
                                params![record.content_hash, payment],
                            )
                            .map_err(PersistenceError::from)?;
                    }
                } else {
                    reused_ad_versions = reused_ad_versions.saturating_add(1);
                }
                transaction
                    .execute(
                        "INSERT INTO snapshot_ad_membership(snapshot_id, user_intent, rank_position, content_hash, observed_at_ms)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            snapshot_id,
                            intent_text(intent),
                            usize_to_i64(rank, "rank position")?,
                            record.content_hash,
                            normalized.ad.observed_at_ms(),
                        ],
                    )
                    .map_err(PersistenceError::from)?;
            }
        }
        if fault == FaultPoint::PublicationAfterAds {
            return Err(PersistenceError::FaultInjected);
        }

        for summary in &input.summaries {
            transaction
                .execute(
                    "INSERT INTO snapshot_summaries(snapshot_id, user_intent, metric_key, value_text, unit)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        snapshot_id,
                        intent_text(summary.intent),
                        summary.metric_key,
                        summary.value.map(ExactDecimal::canonical),
                        summary.unit,
                    ],
                )
                .map_err(PersistenceError::from)?;
        }
        verify_transaction_snapshot_counts(&transaction, &snapshot_id, &input)?;
        transaction.commit().map_err(PersistenceError::from)?;

        Ok(PublicationOutcome {
            snapshot_id,
            context_hash: context_key,
            inserted_ad_versions,
            reused_ad_versions,
            buy_memberships: input.acquisition.buy.ads.len() as u64,
            sell_memberships: input.acquisition.sell.ads.len() as u64,
        })
    }

    pub fn load_snapshot(&self, snapshot_id: &str) -> Result<StoredSnapshot> {
        validate_prefixed_identifier(snapshot_id, "sn", "snapshot ID")?;
        let guard = self.lock_inner()?;
        let connection = connection_ref(&guard)?;
        let snapshot = connection
            .query_row(
                "SELECT s.snapshot_id, c.asset, c.fiat, c.amount_text, c.amount_mode,
                        s.committed_ms, s.provider_adapter_version, s.domain_schema_version,
                        s.calculation_version
                 FROM snapshots s JOIN contexts c ON c.context_hash = s.context_hash
                 WHERE s.snapshot_id = ?1 AND s.completion_state = 'complete'",
                [snapshot_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(PersistenceError::from)?
            .ok_or_else(|| PersistenceError::InvalidInput("snapshot does not exist".to_owned()))?;

        let mut statement = connection
            .prepare(
                "SELECT m.content_hash, a.ad_key, a.merchant_key, m.user_intent,
                        m.rank_position, a.advertiser_side, a.price_text, a.min_fiat_text,
                        a.max_fiat_text, a.available_asset_text, a.monthly_orders,
                        a.completion_percent_text, a.positive_percent_text, a.is_pro,
                        a.merchant_active_seconds, m.observed_at_ms
                 FROM snapshot_ad_membership m
                 JOIN ad_versions a ON a.content_hash = m.content_hash
                 WHERE m.snapshot_id = ?1
                 ORDER BY CASE m.user_intent WHEN 'buy-asset' THEN 0 ELSE 1 END, m.rank_position",
            )
            .map_err(PersistenceError::from)?;
        let rows = statement
            .query_map([snapshot_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, i64>(15)?,
                ))
            })
            .map_err(PersistenceError::from)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(PersistenceError::from)?;
        drop(statement);

        let mut ads = Vec::with_capacity(rows.len());
        for row in rows {
            let payments = load_payments(connection, &row.0)?;
            ads.push(StoredAdVersion {
                content_hash: row.0,
                ad_key: row.1,
                merchant_key: row.2,
                user_intent: parse_intent(&row.3)?,
                rank_position: u32::try_from(row.4).map_err(|_| {
                    PersistenceError::Integrity("invalid stored rank position".to_owned())
                })?,
                advertiser_side: row.5,
                price: parse_stored_decimal(&row.6)?,
                min_fiat: parse_stored_decimal(&row.7)?,
                max_fiat: parse_stored_decimal(&row.8)?,
                available_asset: parse_stored_decimal(&row.9)?,
                payment_methods: payments,
                monthly_orders: u64::try_from(row.10).map_err(|_| {
                    PersistenceError::Integrity("negative stored monthly orders".to_owned())
                })?,
                completion_percent: parse_stored_decimal(&row.11)?,
                positive_percent: parse_stored_decimal(&row.12)?,
                is_pro: row.13 == 1,
                merchant_active_seconds: u64::try_from(row.14).map_err(|_| {
                    PersistenceError::Integrity("negative stored activity time".to_owned())
                })?,
                observed_at_ms: row.15,
            });
        }

        Ok(StoredSnapshot {
            snapshot_id: snapshot.0,
            asset: snapshot.1,
            fiat: snapshot.2,
            amount: parse_stored_decimal(&snapshot.3)?,
            amount_mode: snapshot.4,
            committed_ms: snapshot.5,
            provider_adapter_version: snapshot.6,
            domain_schema_version: snapshot.7,
            calculation_version: snapshot.8,
            ads,
        })
    }

    pub fn insert_rollup(&self, input: RollupInput) -> Result<String> {
        validate_metric_and_unit(&input.metric_key, &input.unit)?;
        let rollup_id = random_identifier("ru")?;
        let guard = self.lock_inner()?;
        connection_ref(&guard)?
            .execute(
                "INSERT INTO history_rollups(
                    rollup_id, asset, fiat, user_intent, period_kind, period_start_ms,
                    metric_key, value_text, unit, sample_count, calculation_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(asset, fiat, user_intent, period_kind, period_start_ms, metric_key, calculation_version)
                 DO UPDATE SET value_text = excluded.value_text, unit = excluded.unit,
                               sample_count = excluded.sample_count",
                params![
                    rollup_id,
                    input.pair.asset().as_str(),
                    input.pair.fiat().as_str(),
                    intent_text(input.intent),
                    input.period.as_str(),
                    input.period_start_ms,
                    input.metric_key,
                    input.value.map(ExactDecimal::canonical),
                    input.unit,
                    u64_to_i64(input.sample_count, "rollup sample count")?,
                    self.versions.calculation_version,
                ],
            )
            .map_err(PersistenceError::from)?;
        Ok(rollup_id)
    }

    pub fn save_setting(
        &self,
        section_key: &str,
        setting_key: &str,
        value: &serde_json::Value,
        updated_at_ms: i64,
    ) -> Result<()> {
        validate_key(section_key, 80, "settings section")?;
        validate_key(setting_key, 80, "setting key")?;
        reject_sensitive_setting(section_key, setting_key, value)?;
        let value_json = serde_json::to_string(value)?;
        if value_json.len() > 262_144 {
            return Err(PersistenceError::InvalidInput(
                "setting JSON exceeds 256 KiB".to_owned(),
            ));
        }
        let guard = self.lock_inner()?;
        connection_ref(&guard)?
            .execute(
                "INSERT INTO settings(section_key, setting_key, value_json, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(section_key, setting_key) DO UPDATE SET
                    value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms",
                params![section_key, setting_key, value_json, updated_at_ms],
            )
            .map_err(PersistenceError::from)?;
        Ok(())
    }

    pub fn load_setting(
        &self,
        section_key: &str,
        setting_key: &str,
    ) -> Result<Option<serde_json::Value>> {
        validate_key(section_key, 80, "settings section")?;
        validate_key(setting_key, 80, "setting key")?;
        let guard = self.lock_inner()?;
        let value = connection_ref(&guard)?
            .query_row(
                "SELECT value_json FROM settings WHERE section_key = ?1 AND setting_key = ?2",
                params![section_key, setting_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(PersistenceError::from)?;
        value
            .map(|json| serde_json::from_str(&json).map_err(PersistenceError::from))
            .transpose()
    }

    pub fn save_catalog_pair(&self, input: CatalogPairInput) -> Result<()> {
        validate_catalog_input(&input)?;
        let pair_key = format!("{}/{}", input.pair.asset(), input.pair.fiat());
        let precision_json = serde_json::to_string(&input.precision)?;
        if precision_json.len() > 65_536 {
            return Err(PersistenceError::InvalidInput(
                "pair precision metadata exceeds 64 KiB".to_owned(),
            ));
        }
        let mut guard = self.lock_inner()?;
        let transaction = connection_mut(&mut guard)?
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;
        transaction
            .execute(
                "INSERT INTO pair_catalog(
                    pair_key, asset, fiat, enabled, disabled_reason, verified_at_ms,
                    disabled_at_ms, provider_adapter_version, precision_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(pair_key) DO UPDATE SET enabled = excluded.enabled,
                    disabled_reason = excluded.disabled_reason, verified_at_ms = excluded.verified_at_ms,
                    disabled_at_ms = excluded.disabled_at_ms,
                    provider_adapter_version = excluded.provider_adapter_version,
                    precision_json = excluded.precision_json",
                params![
                    pair_key,
                    input.pair.asset().as_str(),
                    input.pair.fiat().as_str(),
                    bool_i64(input.enabled),
                    input.disabled_reason,
                    input.verified_at_ms,
                    input.disabled_at_ms,
                    self.versions.provider_adapter_version,
                    precision_json,
                ],
            )
            .map_err(PersistenceError::from)?;
        transaction
            .execute(
                "DELETE FROM pair_catalog_payments WHERE pair_key = ?1",
                [&pair_key],
            )
            .map_err(PersistenceError::from)?;
        for payment in input.payment_methods {
            transaction
                .execute(
                    "INSERT INTO pair_catalog_payments(pair_key, payment_method) VALUES (?1, ?2)",
                    params![pair_key, payment.as_str()],
                )
                .map_err(PersistenceError::from)?;
        }
        transaction.commit().map_err(PersistenceError::from)
    }

    pub fn create_cost_profile_version(
        &self,
        input: CostProfileInput,
    ) -> Result<CostVersionOutcome> {
        validate_cost_input(&input)?;
        let mut guard = self.lock_inner()?;
        let transaction = connection_mut(&mut guard)?
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;
        let existing_profile = transaction
            .query_row(
                "SELECT profile_id FROM cost_profiles
                 WHERE asset = ?1 AND fiat = ?2 AND route_key = ?3 AND leg = ?4
                   AND payment_method = ?5",
                params![
                    input.pair.asset().as_str(),
                    input.pair.fiat().as_str(),
                    input.route_key,
                    intent_text(input.leg),
                    input.payment_method.as_str(),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(PersistenceError::from)?;
        let profile_id = match existing_profile {
            Some(profile_id) => profile_id,
            None => random_identifier("cp")?,
        };
        transaction
            .execute(
                "INSERT OR IGNORE INTO cost_profiles(
                    profile_id, asset, fiat, route_key, leg, payment_method, created_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    profile_id,
                    input.pair.asset().as_str(),
                    input.pair.fiat().as_str(),
                    input.route_key,
                    intent_text(input.leg),
                    input.payment_method.as_str(),
                    input.created_at_ms,
                ],
            )
            .map_err(PersistenceError::from)?;
        let version_id = cost_version_hash(&profile_id, &input);
        transaction
            .execute(
                "INSERT INTO cost_profile_versions(
                    version_id, profile_id, effective_from_ms, effective_to_ms, label,
                    fixed_fiat_text, percent_fiat_text, fixed_asset_text,
                    minimum_charge_text, maximum_charge_text, fixed_buffer_text,
                    percent_buffer_text, source_label, note, created_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    version_id,
                    profile_id,
                    input.effective_from_ms,
                    input.effective_to_ms,
                    input.label,
                    input.fixed_fiat.map(ExactDecimal::canonical),
                    input.percent_fiat.map(ExactDecimal::canonical),
                    input.fixed_asset.map(ExactDecimal::canonical),
                    input.minimum_charge.map(ExactDecimal::canonical),
                    input.maximum_charge.map(ExactDecimal::canonical),
                    input.fixed_buffer.map(ExactDecimal::canonical),
                    input.percent_buffer.map(ExactDecimal::canonical),
                    input.source_label,
                    input.note,
                    input.created_at_ms,
                ],
            )
            .map_err(PersistenceError::from)?;
        transaction.commit().map_err(PersistenceError::from)?;
        Ok(CostVersionOutcome {
            profile_id,
            version_id,
        })
    }

    pub fn save_annotation(&self, input: AnnotationInput) -> Result<String> {
        validate_document_input(
            &input.chart_key,
            input.context_hash.as_deref(),
            &input.payload,
            input.schema_version,
            input.created_at_ms,
            input.updated_at_ms,
        )?;
        let annotation_id = random_identifier("an")?;
        let payload = serde_json::to_string(&input.payload)?;
        let guard = self.lock_inner()?;
        connection_ref(&guard)?
            .execute(
                "INSERT INTO chart_annotations(
                    annotation_id, chart_key, context_hash, payload_json, schema_version,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    annotation_id,
                    input.chart_key,
                    input.context_hash,
                    payload,
                    input.schema_version,
                    input.created_at_ms,
                    input.updated_at_ms,
                ],
            )
            .map_err(PersistenceError::from)?;
        Ok(annotation_id)
    }

    pub fn save_named_view(&self, input: NamedViewInput) -> Result<String> {
        validate_document_input(
            &input.chart_key,
            input.context_hash.as_deref(),
            &input.payload,
            input.schema_version,
            input.created_at_ms,
            input.updated_at_ms,
        )?;
        validate_text(&input.name, 120, "view name")?;
        let view_id = random_identifier("vw")?;
        let payload = serde_json::to_string(&input.payload)?;
        let guard = self.lock_inner()?;
        connection_ref(&guard)?
            .execute(
                "INSERT INTO named_views(
                    view_id, chart_key, name, context_hash, payload_json, schema_version,
                    created_at_ms, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    view_id,
                    input.chart_key,
                    input.name,
                    input.context_hash,
                    payload,
                    input.schema_version,
                    input.created_at_ms,
                    input.updated_at_ms,
                ],
            )
            .map_err(PersistenceError::from)?;
        Ok(view_id)
    }

    pub fn prune(&self, now_ms: i64, policy: RetentionPolicy) -> Result<PruneOutcome> {
        validate_retention_policy(policy)?;
        let before = self.managed_size_bytes()?;
        let mut guard = self.lock_inner()?;
        let connection = connection_mut(&mut guard)?;
        apply_max_page_count(connection, policy.managed_cap_bytes)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;

        let detail_cutoff = now_ms.saturating_sub(policy.detail_retention_ms);
        let summary_cutoff = now_ms.saturating_sub(policy.summary_retention_ms);
        let rollup_cutoff = now_ms.saturating_sub(policy.rollup_retention_ms);
        let detail_snapshots_pruned = count_query(
            &transaction,
            "SELECT COUNT(DISTINCT snapshot_id) FROM snapshot_ad_membership
             WHERE snapshot_id IN (SELECT snapshot_id FROM snapshots WHERE committed_ms < ?1)",
            detail_cutoff,
        )?;
        transaction
            .execute(
                "DELETE FROM snapshot_ad_membership
                 WHERE snapshot_id IN (SELECT snapshot_id FROM snapshots WHERE committed_ms < ?1)",
                [detail_cutoff],
            )
            .map_err(PersistenceError::from)?;
        let orphan_ad_versions_pruned = transaction
            .execute(
                "DELETE FROM ad_versions WHERE content_hash NOT IN
                 (SELECT DISTINCT content_hash FROM snapshot_ad_membership)",
                [],
            )
            .map_err(PersistenceError::from)? as u64;
        let summary_snapshots_pruned = transaction
            .execute(
                "DELETE FROM snapshots WHERE committed_ms < ?1",
                [summary_cutoff],
            )
            .map_err(PersistenceError::from)? as u64;
        transaction
            .execute(
                "DELETE FROM contexts WHERE context_hash NOT IN
                 (SELECT DISTINCT context_hash FROM snapshots)",
                [],
            )
            .map_err(PersistenceError::from)?;
        let rollups_pruned = transaction
            .execute(
                "DELETE FROM history_rollups WHERE period_start_ms < ?1",
                [rollup_cutoff],
            )
            .map_err(PersistenceError::from)? as u64;
        transaction
            .execute(
                "DELETE FROM retention_events WHERE occurred_at_ms < ?1",
                [rollup_cutoff],
            )
            .map_err(PersistenceError::from)?;
        transaction.commit().map_err(PersistenceError::from)?;
        checkpoint_and_reclaim(connection)?;

        drop(guard);
        let cap_detail_snapshots_pruned = self.prune_for_cap(policy.managed_cap_bytes)?;
        let after_before_events = self.managed_size_bytes()?;
        self.record_retention_events(
            now_ms,
            detail_snapshots_pruned,
            summary_snapshots_pruned,
            rollups_pruned,
            cap_detail_snapshots_pruned,
            before,
            after_before_events,
        )?;
        let after = self.managed_size_bytes()?;
        Ok(PruneOutcome {
            detail_snapshots_pruned,
            summary_snapshots_pruned,
            rollups_pruned,
            orphan_ad_versions_pruned,
            cap_detail_snapshots_pruned,
            managed_bytes_before: before,
            managed_bytes_after: after,
            cap_satisfied: after <= policy.managed_cap_bytes,
        })
    }

    pub fn managed_size_bytes(&self) -> Result<u64> {
        let mut total = 0_u64;
        for path in [
            self.database_path.clone(),
            path_with_suffix(&self.database_path, "-wal"),
            path_with_suffix(&self.database_path, "-shm"),
        ] {
            if let Ok(metadata) = path.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
        Ok(total)
    }

    fn prune_for_cap(&self, managed_cap_bytes: u64) -> Result<u64> {
        let mut pruned = 0_u64;
        loop {
            if self.managed_size_bytes()? <= managed_cap_bytes {
                return Ok(pruned);
            }
            let mut guard = self.lock_inner()?;
            let connection = connection_mut(&mut guard)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(PersistenceError::from)?;
            let oldest = transaction
                .query_row(
                    "SELECT s.snapshot_id FROM snapshots s
                     WHERE EXISTS (SELECT 1 FROM snapshot_ad_membership m WHERE m.snapshot_id = s.snapshot_id)
                       AND s.snapshot_id != (SELECT snapshot_id FROM snapshots ORDER BY committed_ms DESC, snapshot_id DESC LIMIT 1)
                     ORDER BY s.committed_ms, s.snapshot_id LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(PersistenceError::from)?;
            let Some(snapshot_id) = oldest else {
                return Ok(pruned);
            };
            transaction
                .execute(
                    "DELETE FROM snapshot_ad_membership WHERE snapshot_id = ?1",
                    [&snapshot_id],
                )
                .map_err(PersistenceError::from)?;
            transaction
                .execute(
                    "DELETE FROM ad_versions WHERE content_hash NOT IN
                     (SELECT DISTINCT content_hash FROM snapshot_ad_membership)",
                    [],
                )
                .map_err(PersistenceError::from)?;
            transaction.commit().map_err(PersistenceError::from)?;
            pruned = pruned.saturating_add(1);
            checkpoint_and_reclaim(connection)?;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_retention_events(
        &self,
        occurred_at_ms: i64,
        detail_snapshots: u64,
        summary_snapshots: u64,
        rollups: u64,
        cap_snapshots: u64,
        managed_bytes_before: u64,
        managed_bytes_after: u64,
    ) -> Result<()> {
        let events = [
            ("expired-detail", detail_snapshots),
            ("expired-summary", summary_snapshots),
            ("expired-rollup", rollups),
            ("managed-cap", cap_snapshots),
        ];
        let mut guard = self.lock_inner()?;
        let transaction = connection_mut(&mut guard)?
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;
        for (reason, affected) in events {
            if affected == 0 {
                continue;
            }
            transaction
                .execute(
                    "INSERT INTO retention_events(
                        event_id, occurred_at_ms, reason, snapshots_affected, rows_deleted,
                        managed_bytes_before, managed_bytes_after
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        random_identifier("re")?,
                        occurred_at_ms,
                        reason,
                        u64_to_i64(affected, "retention affected count")?,
                        u64_to_i64(affected, "retention deleted count")?,
                        u64_to_i64(managed_bytes_before, "managed bytes before")?,
                        u64_to_i64(managed_bytes_after, "managed bytes after")?,
                    ],
                )
                .map_err(PersistenceError::from)?;
        }
        transaction.commit().map_err(PersistenceError::from)
    }

    pub fn create_backup(
        &self,
        destination: impl AsRef<Path>,
        created_at_ms: i64,
    ) -> Result<BackupOutcome> {
        let guard = self.lock_inner()?;
        let connection = connection_ref(&guard)?;
        checkpoint(connection)?;
        create_backup_archive(
            connection,
            &self.identity_key_path,
            destination.as_ref(),
            created_at_ms,
            &self.versions,
        )
    }

    pub fn restore_backup(
        &self,
        archive_path: impl AsRef<Path>,
        restored_at_ms: i64,
    ) -> Result<RestoreOutcome> {
        self.restore_with_fault(archive_path.as_ref(), restored_at_ms, self.options.fault)
    }

    fn restore_with_fault(
        &self,
        archive_path: &Path,
        restored_at_ms: i64,
        fault: FaultPoint,
    ) -> Result<RestoreOutcome> {
        let extracted = validate_and_extract_backup(archive_path, &self.temporary_directory)?;
        validate_database_file(
            &extracted.database_path,
            self.options.busy_timeout_ms,
            extracted.manifest.database_schema_version,
        )?;
        let required = fs::metadata(&extracted.database_path)?
            .len()
            .saturating_mul(2)
            .saturating_add(1024 * 1024);
        let available = self
            .options
            .available_space_override
            .unwrap_or(fs2::available_space(&self.root)?);
        if available < required {
            return Err(PersistenceError::InvalidBackup(
                "insufficient free space for validated atomic restore and rollback".to_owned(),
            ));
        }

        let mut guard = self.lock_inner()?;
        let current = connection_ref(&guard)?;
        checkpoint(current)?;
        let safety_path = unique_automatic_backup_path(
            &self.automatic_backup_directory,
            restored_at_ms,
            schema_version(current)?,
            "restore-safety",
        )?;
        let safety = create_backup_archive(
            current,
            &self.identity_key_path,
            &safety_path,
            restored_at_ms,
            &self.versions,
        )?;
        retain_latest_automatic_backups(&self.automatic_backup_directory)?;

        let rollback_database = restore_rollback_database_path(&self.database_path);
        let rollback_key = restore_rollback_key_path(&self.identity_key_path);
        let staged_database = restore_staged_database_path(&self.database_path);
        let staged_key = restore_staged_key_path(&self.identity_key_path);
        let restore_marker = restore_marker_path(&self.database_path)?;
        copy_synced(&extracted.database_path, &staged_database)?;
        copy_synced(&extracted.identity_key_path, &staged_key)?;
        create_restore_marker(&restore_marker, restored_at_ms)?;
        if let Err(error) = close_guard_connection(&mut guard) {
            remove_if_exists(&staged_database)?;
            remove_if_exists(&staged_key)?;
            complete_restore_marker(&restore_marker)?;
            return Err(error);
        }
        if let Err(error) = remove_sidecars(&self.database_path) {
            return rollback_restore_swap(
                &mut guard,
                &self.database_path,
                &self.identity_key_path,
                &rollback_database,
                &rollback_key,
                &staged_database,
                &staged_key,
                &restore_marker,
                self.options.busy_timeout_ms,
                error,
            );
        }
        let swap_result = (|| -> Result<()> {
            fs::rename(&self.database_path, &rollback_database)?;
            if fault == FaultPoint::RestoreAfterDatabaseMoved {
                return Err(PersistenceError::FaultInjected);
            }
            fs::rename(&self.identity_key_path, &rollback_key)?;
            if fault == FaultPoint::RestoreAfterKeyMoved {
                return Err(PersistenceError::FaultInjected);
            }
            fs::rename(&staged_database, &self.database_path)?;
            if fault == FaultPoint::RestoreAfterDatabaseInstalled {
                return Err(PersistenceError::FaultInjected);
            }
            fs::rename(&staged_key, &self.identity_key_path)?;
            Ok(())
        })();
        if let Err(error) = swap_result {
            return rollback_restore_swap(
                &mut guard,
                &self.database_path,
                &self.identity_key_path,
                &rollback_database,
                &rollback_key,
                &staged_database,
                &staged_key,
                &restore_marker,
                self.options.busy_timeout_ms,
                error,
            );
        }

        let restored_schema = extracted.manifest.database_schema_version;
        let replacement_result = (|| -> Result<(Connection, [u8; 32])> {
            if fault == FaultPoint::RestoreAfterReplacement {
                return Err(PersistenceError::FaultInjected);
            }
            let key = read_identity_key(&self.identity_key_path)?;
            let mut connection =
                open_connection(&self.database_path, self.options.busy_timeout_ms)?;
            check_integrity(&connection, true)?;
            let existing = schema_version(&connection)?;
            migrate_database(
                &mut connection,
                &self.identity_key_path,
                &self.automatic_backup_directory,
                &self.versions,
                restored_at_ms,
                true,
                FaultPoint::None,
            )?;
            ensure_supported_schema(existing)?;
            verify_migration_catalog(&connection)?;
            validate_compiled_schema(&connection)?;
            validate_semantic_storage(&connection)?;
            check_integrity(&connection, true)?;
            apply_max_page_count(&connection, RetentionPolicy::default().managed_cap_bytes)?;
            write_runtime_metadata(&connection, &self.versions, restored_at_ms)?;
            Ok((connection, key))
        })();

        match replacement_result {
            Ok((connection, key)) => {
                if let Err(error) = complete_restore_marker(&restore_marker) {
                    drop(connection);
                    return rollback_restore_swap(
                        &mut guard,
                        &self.database_path,
                        &self.identity_key_path,
                        &rollback_database,
                        &rollback_key,
                        &staged_database,
                        &staged_key,
                        &restore_marker,
                        self.options.busy_timeout_ms,
                        error,
                    );
                }
                guard.connection = Some(connection);
                guard.identity_key = key;
                remove_if_exists(&rollback_database)?;
                remove_if_exists(&rollback_key)?;
                retain_latest_automatic_backups(&self.automatic_backup_directory)?;
                Ok(RestoreOutcome {
                    restored_schema_version: restored_schema,
                    migrated_to_schema_version: DATABASE_SCHEMA_VERSION,
                    safety_backup_path: safety.path,
                })
            }
            Err(error) => rollback_restore_swap(
                &mut guard,
                &self.database_path,
                &self.identity_key_path,
                &rollback_database,
                &rollback_key,
                &staged_database,
                &staged_key,
                &restore_marker,
                self.options.busy_timeout_ms,
                error,
            ),
        }
    }

    pub fn clear(&self, scope: ClearScope) -> Result<ClearOutcome> {
        let database_rows_deleted = match scope {
            ClearScope::Logs => 0,
            ClearScope::History => self.clear_database_tables(&[
                "snapshot_ad_membership",
                "snapshot_summaries",
                "snapshot_pages",
                "snapshots",
                "ad_versions",
                "contexts",
                "history_rollups",
                "retention_events",
            ])?,
            ClearScope::AnnotationsAndViews => {
                self.clear_database_tables(&["chart_annotations", "named_views"])?
            }
            ClearScope::Settings => self.clear_database_tables(&["settings"])?,
            ClearScope::AllLocalData => self.clear_database_tables(&[
                "report_audit",
                "diagnostic_index",
                "retention_events",
                "snapshot_ad_membership",
                "snapshot_summaries",
                "snapshot_pages",
                "snapshots",
                "ad_versions",
                "contexts",
                "history_rollups",
                "cost_profile_versions",
                "cost_profiles",
                "pair_catalog_payments",
                "pair_catalog",
                "chart_annotations",
                "named_views",
                "settings",
            ])?,
        };
        let filesystem_entries_deleted = match scope {
            ClearScope::Logs => clear_directory(&self.root.join(LOG_DIRECTORY))?,
            ClearScope::AllLocalData => {
                let logs = clear_directory(&self.root.join(LOG_DIRECTORY))?;
                let crash = clear_directory(&self.root.join(CRASH_DIRECTORY))?;
                let temporary = clear_directory(&self.temporary_directory)?;
                let backups = clear_directory(&self.automatic_backup_directory)?;
                logs.saturating_add(crash)
                    .saturating_add(temporary)
                    .saturating_add(backups)
            }
            _ => 0,
        };
        Ok(ClearOutcome {
            scope,
            database_rows_deleted,
            filesystem_entries_deleted,
        })
    }

    fn clear_database_tables(&self, tables: &[&str]) -> Result<u64> {
        let mut guard = self.lock_inner()?;
        let transaction = connection_mut(&mut guard)?
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(PersistenceError::from)?;
        let mut deleted = 0_u64;
        for table in tables {
            let sql = format!("DELETE FROM {table}");
            deleted = deleted.saturating_add(
                transaction
                    .execute(&sql, [])
                    .map_err(PersistenceError::from)? as u64,
            );
        }
        transaction.commit().map_err(PersistenceError::from)?;
        Ok(deleted)
    }

    fn lock_inner(&self) -> Result<MutexGuard<'_, StoreInner>> {
        self.inner
            .lock()
            .map_err(|_| PersistenceError::Sqlite("persistence mutex is poisoned".to_owned()))
    }
}

fn create_data_directories(root: &Path) -> Result<()> {
    for directory in [
        root.to_path_buf(),
        root.join(DATABASE_DIRECTORY),
        root.join(AUTOMATIC_BACKUP_DIRECTORY),
        root.join(TEMPORARY_DIRECTORY),
        root.join(LOG_DIRECTORY),
        root.join(CRASH_DIRECTORY),
    ] {
        fs::create_dir_all(directory)?;
    }
    Ok(())
}

fn load_or_create_identity_key(path: &Path, existing_database: bool) -> Result<[u8; 32]> {
    if path.exists() {
        return read_identity_key(path);
    }
    if existing_database {
        return Err(PersistenceError::InvalidIdentityKey);
    }
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key).map_err(|error| PersistenceError::Entropy(error.to_string()))?;
    let mut options = FileOpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&key)?;
    file.sync_all()?;
    Ok(key)
}

fn read_identity_key(path: &Path) -> Result<[u8; 32]> {
    secure_identity_key_permissions(path)?;
    let bytes = fs::read(path)?;
    bytes
        .try_into()
        .map_err(|_| PersistenceError::InvalidIdentityKey)
}

fn secure_identity_key_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn open_connection(path: &Path, busy_timeout_ms: u64) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(PersistenceError::from)?;
    configure_connection(&connection, busy_timeout_ms)?;
    Ok(connection)
}

fn configure_connection(connection: &Connection, busy_timeout_ms: u64) -> Result<()> {
    connection
        .busy_timeout(Duration::from_millis(busy_timeout_ms))
        .map_err(PersistenceError::from)?;
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA wal_autocheckpoint = 1000;
             PRAGMA trusted_schema = OFF;
             PRAGMA temp_store = MEMORY;",
        )
        .map_err(PersistenceError::from)?;
    let foreign_keys: i64 = connection
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(PersistenceError::from)?;
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(PersistenceError::from)?;
    if foreign_keys != 1 || !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(PersistenceError::Integrity(
            "required foreign-key or WAL mode could not be enabled".to_owned(),
        ));
    }
    Ok(())
}

fn migrate_database(
    connection: &mut Connection,
    identity_key_path: &Path,
    automatic_backup_directory: &Path,
    versions: &RuntimeVersions,
    migrated_at_ms: i64,
    existing_database: bool,
    fault: FaultPoint,
) -> Result<()> {
    let current = schema_version(connection)?;
    ensure_supported_schema(current)?;
    let pending = MIGRATIONS
        .iter()
        .filter(|migration| migration.version > current)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(());
    }
    if !existing_database && current == 0 {
        connection
            .execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")
            .map_err(PersistenceError::from)?;
    }

    let mut version_before_migration = current;
    for migration in pending {
        if existing_database {
            checkpoint(connection)?;
            let path = unique_automatic_backup_path(
                automatic_backup_directory,
                migrated_at_ms,
                version_before_migration,
                "migration-backup",
            )?;
            create_backup_archive(
                connection,
                identity_key_path,
                &path,
                migrated_at_ms,
                versions,
            )?;
        }
        apply_migration(connection, migration, migrated_at_ms, fault)?;
        version_before_migration = migration.version;
    }
    Ok(())
}

fn apply_migration(
    connection: &mut Connection,
    migration: &Migration,
    migrated_at_ms: i64,
    fault: FaultPoint,
) -> Result<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Exclusive)
        .map_err(PersistenceError::from)?;
    transaction
        .execute_batch(migration.sql)
        .map_err(PersistenceError::from)?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version, name, checksum_sha256, applied_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                i64::from(migration.version),
                migration.name,
                migration_checksum(migration),
                migrated_at_ms,
            ],
        )
        .map_err(PersistenceError::from)?;
    transaction
        .execute_batch(&format!("PRAGMA user_version = {};", migration.version))
        .map_err(PersistenceError::from)?;
    if fault == FaultPoint::MigrationBeforeCommit {
        return Err(PersistenceError::FaultInjected);
    }
    transaction.commit().map_err(PersistenceError::from)
}

fn verify_migration_catalog(connection: &Connection) -> Result<()> {
    let current = schema_version(connection)?;
    if current != DATABASE_SCHEMA_VERSION {
        return Err(PersistenceError::Incompatible(format!(
            "expected schema {DATABASE_SCHEMA_VERSION}, found {current}"
        )));
    }
    for migration in MIGRATIONS {
        let stored = connection
            .query_row(
                "SELECT name, checksum_sha256 FROM schema_migrations WHERE version = ?1",
                [i64::from(migration.version)],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(PersistenceError::from)?;
        if stored != Some((migration.name.to_owned(), migration_checksum(migration))) {
            return Err(PersistenceError::Integrity(format!(
                "migration {} does not match the compiled catalog",
                migration.version
            )));
        }
    }
    Ok(())
}

fn validate_compiled_schema(connection: &Connection) -> Result<()> {
    let expected = Connection::open_in_memory().map_err(PersistenceError::from)?;
    for migration in MIGRATIONS {
        expected
            .execute_batch(migration.sql)
            .map_err(PersistenceError::from)?;
    }
    if schema_records(connection)? != schema_records(&expected)? {
        return Err(PersistenceError::Integrity(
            "database objects do not exactly match the compiled schema".to_owned(),
        ));
    }
    Ok(())
}

fn schema_records(connection: &Connection) -> Result<Vec<(String, String, String, String)>> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, COALESCE(sql, '') FROM sqlite_schema
             WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .map_err(PersistenceError::from)?;
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(PersistenceError::from)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)
}

fn write_runtime_metadata(
    connection: &Connection,
    versions: &RuntimeVersions,
    updated_at_ms: i64,
) -> Result<()> {
    for (key, value) in [
        ("app-version", versions.app_version.as_str()),
        (
            "provider-adapter-version",
            versions.provider_adapter_version.as_str(),
        ),
        (
            "domain-schema-version",
            versions.domain_schema_version.as_str(),
        ),
        ("calculation-version", versions.calculation_version.as_str()),
    ] {
        connection
            .execute(
                "INSERT INTO metadata(key, value, updated_at_ms) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                    updated_at_ms = excluded.updated_at_ms",
                params![key, value, updated_at_ms],
            )
            .map_err(PersistenceError::from)?;
    }
    Ok(())
}

fn check_integrity(connection: &Connection, full: bool) -> Result<()> {
    let pragma = if full {
        "PRAGMA integrity_check"
    } else {
        "PRAGMA quick_check"
    };
    let mut statement = connection.prepare(pragma).map_err(PersistenceError::from)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(PersistenceError::from)?;
    let messages = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)?;
    if messages.len() != 1 || messages[0] != "ok" {
        return Err(PersistenceError::Integrity(messages.join("; ")));
    }
    Ok(())
}

fn validate_semantic_storage(connection: &Connection) -> Result<()> {
    let mut table_statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(PersistenceError::from)?;
    let tables = table_statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(PersistenceError::from)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)?;
    drop(table_statement);

    let mut column_statement = connection
        .prepare("SELECT name, type FROM pragma_table_info(?1)")
        .map_err(PersistenceError::from)?;
    for table in tables {
        let columns = column_statement
            .query_map([table.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(PersistenceError::from)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(PersistenceError::from)?;
        for (name, declared_type) in columns {
            if declared_type.eq_ignore_ascii_case("REAL") {
                return Err(PersistenceError::Integrity(format!(
                    "SQLite REAL column {table}.{name} is prohibited"
                )));
            }
            if name.ends_with("_text") && !declared_type.eq_ignore_ascii_case("TEXT") {
                return Err(PersistenceError::Integrity(format!(
                    "exact decimal column {table}.{name} is not TEXT"
                )));
            }
            let lower = name.to_ascii_lowercase();
            if lower.contains("nickname")
                || lower.contains("raw_body")
                || lower.contains("raw_response")
                || lower.contains("provider_payload")
            {
                return Err(PersistenceError::Integrity(format!(
                    "forbidden storage column {table}.{name} exists"
                )));
            }
        }
    }
    validate_settings_boundary(connection)?;
    validate_json_query(
        connection,
        "SELECT selected_payments_json FROM contexts
         UNION ALL SELECT buy_rejection_counts_json FROM snapshots
         UNION ALL SELECT sell_rejection_counts_json FROM snapshots
         UNION ALL SELECT precision_json FROM pair_catalog
         UNION ALL SELECT payload_json FROM chart_annotations
         UNION ALL SELECT payload_json FROM named_views",
    )?;
    validate_decimal_query(
        connection,
        "SELECT amount_text FROM contexts
         UNION ALL SELECT minimum_completion_percent_text FROM contexts
         UNION ALL SELECT minimum_positive_percent_text FROM contexts
         UNION ALL SELECT maximum_buy_price_text FROM contexts WHERE maximum_buy_price_text IS NOT NULL
         UNION ALL SELECT minimum_sell_price_text FROM contexts WHERE minimum_sell_price_text IS NOT NULL
         UNION ALL SELECT price_text FROM ad_versions
         UNION ALL SELECT min_fiat_text FROM ad_versions
         UNION ALL SELECT max_fiat_text FROM ad_versions
         UNION ALL SELECT available_asset_text FROM ad_versions
         UNION ALL SELECT completion_percent_text FROM ad_versions
         UNION ALL SELECT positive_percent_text FROM ad_versions
         UNION ALL SELECT value_text FROM snapshot_summaries WHERE value_text IS NOT NULL
         UNION ALL SELECT value_text FROM history_rollups WHERE value_text IS NOT NULL
         UNION ALL SELECT fixed_fiat_text FROM cost_profile_versions WHERE fixed_fiat_text IS NOT NULL
         UNION ALL SELECT percent_fiat_text FROM cost_profile_versions WHERE percent_fiat_text IS NOT NULL
         UNION ALL SELECT fixed_asset_text FROM cost_profile_versions WHERE fixed_asset_text IS NOT NULL
         UNION ALL SELECT minimum_charge_text FROM cost_profile_versions WHERE minimum_charge_text IS NOT NULL
         UNION ALL SELECT maximum_charge_text FROM cost_profile_versions WHERE maximum_charge_text IS NOT NULL
         UNION ALL SELECT fixed_buffer_text FROM cost_profile_versions WHERE fixed_buffer_text IS NOT NULL
         UNION ALL SELECT percent_buffer_text FROM cost_profile_versions WHERE percent_buffer_text IS NOT NULL",
    )?;
    Ok(())
}

fn validate_settings_boundary(connection: &Connection) -> Result<()> {
    let mut statement = connection
        .prepare("SELECT section_key, setting_key, value_json FROM settings")
        .map_err(PersistenceError::from)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(PersistenceError::from)?;
    for row in rows {
        let (section, key, value_json) = row.map_err(PersistenceError::from)?;
        let value: serde_json::Value = serde_json::from_str(&value_json).map_err(|error| {
            PersistenceError::Integrity(format!("invalid settings JSON: {error}"))
        })?;
        reject_sensitive_setting(&section, &key, &value).map_err(|error| {
            PersistenceError::Integrity(format!(
                "stored setting violates product boundary: {error}"
            ))
        })?;
    }
    Ok(())
}

fn validate_json_query(connection: &Connection, sql: &str) -> Result<()> {
    let mut statement = connection.prepare(sql).map_err(PersistenceError::from)?;
    let values = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(PersistenceError::from)?;
    for value in values {
        serde_json::from_str::<serde_json::Value>(&value.map_err(PersistenceError::from)?)
            .map_err(|error| {
                PersistenceError::Integrity(format!("invalid stored JSON: {error}"))
            })?;
    }
    Ok(())
}

fn validate_decimal_query(connection: &Connection, sql: &str) -> Result<()> {
    let mut statement = connection.prepare(sql).map_err(PersistenceError::from)?;
    let values = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(PersistenceError::from)?;
    for value in values {
        parse_stored_decimal(&value.map_err(PersistenceError::from)?)?;
    }
    Ok(())
}

fn validate_publication(input: &PublicationInput<'_>) -> Result<()> {
    if input.acquisition.pair != input.context.pair {
        return Err(PersistenceError::NotPublishable(
            "acquisition pair and applied context differ".to_owned(),
        ));
    }
    if input.acquisition.buy.quality.target() != u32::from(input.context.result_target.value())
        || input.acquisition.sell.quality.target() != u32::from(input.context.result_target.value())
        || !input.acquisition.buy.quality.complete()
        || !input.acquisition.sell.quality.complete()
    {
        return Err(PersistenceError::NotPublishable(
            "both side qualities must be complete for the applied target".to_owned(),
        ));
    }
    if input.acquisition.buy.ads.len() as u32 != input.acquisition.buy.quality.valid()
        || input.acquisition.sell.ads.len() as u32 != input.acquisition.sell.quality.valid()
    {
        return Err(PersistenceError::NotPublishable(
            "validated ad counts do not match side quality".to_owned(),
        ));
    }
    if input
        .acquisition
        .buy
        .ads
        .iter()
        .any(|ad| !ad_matches_intent(&ad.ad, UserIntent::BuyAsset))
        || input
            .acquisition
            .sell
            .ads
            .iter()
            .any(|ad| !ad_matches_intent(&ad.ad, UserIntent::SellAsset))
    {
        return Err(PersistenceError::NotPublishable(
            "a validated ad is assigned to the wrong user intent".to_owned(),
        ));
    }
    let timestamps = ObservationTimestamps::new(
        input.request_started_ms,
        input.last_page_received_ms,
        input.validated_ms,
        input.committed_ms,
        input.agent_checked_ms,
    )
    .map_err(|error| PersistenceError::NotPublishable(error.to_string()))?;
    let provenance = SnapshotProvenance::new(
        StableId::new(input.versions_adapter_placeholder())
            .map_err(|error| PersistenceError::NotPublishable(error.to_string()))?,
        input.acquisition.request_id.clone(),
        timestamps,
        input.acquisition.page_receipts.clone(),
        input.acquisition.buy.quality,
        input.acquisition.sell.quality,
    )
    .map_err(|error| PersistenceError::NotPublishable(error.to_string()))?;
    if !provenance
        .publishable(input.committed_ms, input.refresh_interval_seconds)
        .map_err(|error| PersistenceError::NotPublishable(error.to_string()))?
    {
        return Err(PersistenceError::NotPublishable(
            "snapshot is stale or incomplete at commit".to_owned(),
        ));
    }
    let mut has_buy_summary = false;
    let mut has_sell_summary = false;
    let mut summary_keys = std::collections::BTreeSet::new();
    for summary in &input.summaries {
        validate_metric_and_unit(&summary.metric_key, &summary.unit)?;
        if !summary_keys.insert((intent_text(summary.intent), summary.metric_key.as_str())) {
            return Err(PersistenceError::NotPublishable(
                "summary metric keys must be unique within each side".to_owned(),
            ));
        }
        match summary.intent {
            UserIntent::BuyAsset => has_buy_summary = true,
            UserIntent::SellAsset => has_sell_summary = true,
        }
    }
    if !has_buy_summary || !has_sell_summary {
        return Err(PersistenceError::NotPublishable(
            "at least one summary metric is required for each side".to_owned(),
        ));
    }
    Ok(())
}

trait PublicationAdapterVersion {
    fn versions_adapter_placeholder(&self) -> String;
}

impl PublicationAdapterVersion for PublicationInput<'_> {
    fn versions_adapter_placeholder(&self) -> String {
        p2p_provider::ADAPTER_VERSION.to_owned()
    }
}

fn insert_context(transaction: &Transaction<'_>, input: &PublicationInput<'_>) -> Result<String> {
    let key = context_hash(&input.context);
    let selected_payments = input
        .context
        .filters
        .selected_payments()
        .iter()
        .map(|method| method.as_str())
        .collect::<Vec<_>>();
    let selected_payments_json = serde_json::to_string(&selected_payments)?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO contexts(
                context_hash, asset, fiat, amount_text, amount_mode,
                selected_payments_json, payment_logic, minimum_orders,
                minimum_completion_percent_text, minimum_positive_percent_text,
                pro_only, maximum_buy_price_text, minimum_sell_price_text, result_target
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                key,
                input.context.pair.asset().as_str(),
                input.context.pair.fiat().as_str(),
                input.context.amount.value().canonical(),
                match input.context.amount.mode() {
                    AmountMode::Fiat => "fiat",
                    AmountMode::Asset => "asset",
                },
                selected_payments_json,
                match input.context.filters.payment_logic() {
                    PaymentLogic::Any => "ANY",
                    PaymentLogic::All => "ALL",
                },
                u64_to_i64(input.context.filters.minimum_orders(), "minimum orders")?,
                input
                    .context
                    .filters
                    .minimum_completion_percent()
                    .canonical(),
                input.context.filters.minimum_positive_percent().canonical(),
                bool_i64(input.context.filters.pro_only()),
                input
                    .context
                    .filters
                    .maximum_buy_price()
                    .map(ExactDecimal::canonical),
                input
                    .context
                    .filters
                    .minimum_sell_price()
                    .map(ExactDecimal::canonical),
                i64::from(input.context.result_target.value()),
            ],
        )
        .map_err(PersistenceError::from)?;
    Ok(key)
}

fn insert_snapshot_header(
    transaction: &Transaction<'_>,
    snapshot_id: &str,
    request_key: &str,
    context_key: &str,
    input: &PublicationInput<'_>,
    versions: &RuntimeVersions,
) -> Result<()> {
    let buy = input.acquisition.buy.quality;
    let sell = input.acquisition.sell.quality;
    transaction
        .execute(
            "INSERT INTO snapshots(
                snapshot_id, request_key, context_hash, source_kind,
                provider_adapter_version, domain_schema_version, calculation_version,
                app_version, request_started_ms, last_page_received_ms, validated_ms,
                committed_ms, agent_checked_ms, buy_fetched, buy_valid, buy_duplicates,
                buy_rejected, buy_target, buy_provider_total, buy_exhausted,
                sell_fetched, sell_valid, sell_duplicates, sell_rejected, sell_target,
                sell_provider_total, sell_exhausted, buy_rejection_counts_json,
                sell_rejection_counts_json, completion_state
             ) VALUES (
                ?1, ?2, ?3, 'experimental-binance-p2p-web', ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, 'complete'
             )",
            params![
                snapshot_id,
                request_key,
                context_key,
                versions.provider_adapter_version,
                versions.domain_schema_version,
                versions.calculation_version,
                versions.app_version,
                input.request_started_ms,
                input.last_page_received_ms,
                input.validated_ms,
                input.committed_ms,
                input.agent_checked_ms,
                i64::from(buy.fetched()),
                i64::from(buy.valid()),
                i64::from(buy.duplicates()),
                i64::from(buy.rejected()),
                i64::from(buy.target()),
                buy.provider_total().map(i64::from),
                bool_i64(buy.exhausted()),
                i64::from(sell.fetched()),
                i64::from(sell.valid()),
                i64::from(sell.duplicates()),
                i64::from(sell.rejected()),
                i64::from(sell.target()),
                sell.provider_total().map(i64::from),
                bool_i64(sell.exhausted()),
                serde_json::to_string(&input.acquisition.buy.rejection_counts)?,
                serde_json::to_string(&input.acquisition.sell.rejection_counts)?,
            ],
        )
        .map_err(PersistenceError::from)?;
    Ok(())
}

fn verify_transaction_snapshot_counts(
    transaction: &Transaction<'_>,
    snapshot_id: &str,
    input: &PublicationInput<'_>,
) -> Result<()> {
    for (intent, expected) in [
        (UserIntent::BuyAsset, input.acquisition.buy.ads.len()),
        (UserIntent::SellAsset, input.acquisition.sell.ads.len()),
    ] {
        let stored: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM snapshot_ad_membership
                 WHERE snapshot_id = ?1 AND user_intent = ?2",
                params![snapshot_id, intent_text(intent)],
                |row| row.get(0),
            )
            .map_err(PersistenceError::from)?;
        if stored != usize_to_i64(expected, "membership count")? {
            return Err(PersistenceError::Integrity(
                "snapshot membership count changed inside its transaction".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_database_file(
    path: &Path,
    busy_timeout_ms: u64,
    manifest_schema_version: u32,
) -> Result<()> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(PersistenceError::from)?;
    connection
        .busy_timeout(Duration::from_millis(busy_timeout_ms))
        .map_err(PersistenceError::from)?;
    check_integrity(&connection, true)?;
    let version = schema_version(&connection)?;
    ensure_supported_schema(version)?;
    if version != manifest_schema_version {
        return Err(PersistenceError::InvalidBackup(
            "manifest schema version does not match the database".to_owned(),
        ));
    }
    if version == DATABASE_SCHEMA_VERSION {
        verify_migration_catalog(&connection)?;
        validate_semantic_storage(&connection)?;
    }
    Ok(())
}

fn apply_max_page_count(connection: &Connection, cap_bytes: u64) -> Result<()> {
    let page_size: i64 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(PersistenceError::from)?;
    let page_size = u64::try_from(page_size)
        .map_err(|_| PersistenceError::Integrity("invalid SQLite page size".to_owned()))?;
    let pages = cap_bytes
        .checked_div(page_size)
        .unwrap_or(0)
        .max(1)
        .min(i64::MAX as u64);
    connection
        .execute_batch(&format!("PRAGMA max_page_count = {pages};"))
        .map_err(PersistenceError::from)
}

fn checkpoint(connection: &Connection) -> Result<()> {
    connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(PersistenceError::from)
}

fn checkpoint_and_reclaim(connection: &Connection) -> Result<()> {
    checkpoint(connection)?;
    connection
        .execute_batch("PRAGMA incremental_vacuum(10000);")
        .map_err(PersistenceError::from)?;
    checkpoint(connection)
}

fn count_query(transaction: &Transaction<'_>, sql: &str, value: i64) -> Result<u64> {
    let count: i64 = transaction
        .query_row(sql, [value], |row| row.get(0))
        .map_err(PersistenceError::from)?;
    u64::try_from(count).map_err(|_| PersistenceError::Integrity("negative row count".to_owned()))
}

fn validate_retention_policy(policy: RetentionPolicy) -> Result<()> {
    let cap_is_valid = if cfg!(test) {
        policy.managed_cap_bytes > 0
    } else {
        (RetentionPolicy::MINIMUM_CAP_BYTES..=RetentionPolicy::MAXIMUM_CAP_BYTES)
            .contains(&policy.managed_cap_bytes)
    };
    if policy.detail_retention_ms <= 0
        || policy.summary_retention_ms < policy.detail_retention_ms
        || policy.rollup_retention_ms < policy.summary_retention_ms
        || !cap_is_valid
    {
        return Err(PersistenceError::InvalidInput(
            "invalid retention policy".to_owned(),
        ));
    }
    Ok(())
}

fn validate_cost_input(input: &CostProfileInput) -> Result<()> {
    validate_key(&input.route_key, 200, "cost route key")?;
    validate_text(&input.label, 120, "cost profile label")?;
    if input
        .effective_to_ms
        .is_some_and(|value| value <= input.effective_from_ms)
    {
        return Err(PersistenceError::InvalidInput(
            "cost effective-to time must follow effective-from time".to_owned(),
        ));
    }
    for (name, value) in [
        ("fixed fiat", input.fixed_fiat),
        ("percent fiat", input.percent_fiat),
        ("fixed asset", input.fixed_asset),
        ("minimum charge", input.minimum_charge),
        ("maximum charge", input.maximum_charge),
        ("fixed buffer", input.fixed_buffer),
        ("percent buffer", input.percent_buffer),
    ] {
        if value.is_some_and(ExactDecimal::is_negative) {
            return Err(PersistenceError::InvalidInput(format!(
                "{name} cannot be negative"
            )));
        }
    }
    for (name, value) in [
        ("percent fiat", input.percent_fiat),
        ("percent buffer", input.percent_buffer),
    ] {
        if value.is_some_and(|value| value > ExactDecimal::HUNDRED) {
            return Err(PersistenceError::InvalidInput(format!(
                "{name} cannot exceed 100 percent"
            )));
        }
    }
    if let (Some(minimum), Some(maximum)) = (input.minimum_charge, input.maximum_charge)
        && maximum < minimum
    {
        return Err(PersistenceError::InvalidInput(
            "maximum charge cannot be lower than minimum charge".to_owned(),
        ));
    }
    if let Some(source) = &input.source_label {
        validate_text(source, 200, "cost source")?;
    }
    if let Some(note) = &input.note {
        validate_text(note, 2_000, "cost note")?;
    }
    Ok(())
}

fn validate_catalog_input(input: &CatalogPairInput) -> Result<()> {
    match (
        input.enabled,
        input.disabled_reason.as_deref(),
        input.disabled_at_ms,
    ) {
        (true, None, None) => {}
        (false, Some(reason), Some(_)) => validate_text(reason, 500, "disabled reason")?,
        _ => {
            return Err(PersistenceError::InvalidInput(
                "enabled pairs cannot have disabled state; disabled pairs require reason and time"
                    .to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_document_input(
    chart_key: &str,
    context_hash: Option<&str>,
    payload: &serde_json::Value,
    schema_version: u32,
    created_at_ms: i64,
    updated_at_ms: i64,
) -> Result<()> {
    validate_key(chart_key, 120, "chart key")?;
    if let Some(hash) = context_hash {
        validate_fixed_identifier(hash, 64, "context hash")?;
    }
    if schema_version == 0 || updated_at_ms < created_at_ms {
        return Err(PersistenceError::InvalidInput(
            "document schema and timestamps are invalid".to_owned(),
        ));
    }
    if serde_json::to_vec(payload)?.len() > 1_048_576 {
        return Err(PersistenceError::InvalidInput(
            "document payload exceeds 1 MiB".to_owned(),
        ));
    }
    Ok(())
}

fn validate_metric_and_unit(metric: &str, unit: &str) -> Result<()> {
    validate_key(metric, 80, "metric key")?;
    validate_text(unit, 40, "metric unit")
}

fn validate_key(value: &str, maximum: usize, name: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > maximum
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
    {
        return Err(PersistenceError::InvalidInput(format!(
            "{name} contains invalid characters or length"
        )));
    }
    Ok(())
}

fn reject_sensitive_setting(
    section_key: &str,
    setting_key: &str,
    value: &serde_json::Value,
) -> Result<()> {
    if sensitive_name(section_key) || sensitive_name(setting_key) || json_has_sensitive_key(value) {
        return Err(PersistenceError::InvalidInput(
            "credentials, tokens, passwords, secrets, and account keys are outside the product boundary"
                .to_owned(),
        ));
    }
    Ok(())
}

fn json_has_sensitive_key(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(key, value)| sensitive_name(key) || json_has_sensitive_key(value)),
        serde_json::Value::Array(values) => values.iter().any(json_has_sensitive_key),
        _ => false,
    }
}

fn sensitive_name(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase().replace(['_', '.'], "-");
    [
        "credential",
        "password",
        "passwd",
        "secret",
        "token",
        "account",
        "authentication",
        "authorization",
        "api-key",
        "apikey",
        "access-token",
        "accesstoken",
        "refresh-token",
        "refreshtoken",
        "private-key",
        "privatekey",
        "account-key",
        "accountkey",
    ]
    .iter()
    .any(|token| normalized.contains(token))
}

fn validate_text(value: &str, maximum: usize, name: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > maximum
        || value.chars().any(|character| character.is_control())
    {
        return Err(PersistenceError::InvalidInput(format!(
            "{name} must be nonempty printable text up to {maximum} bytes"
        )));
    }
    Ok(())
}

fn validate_fixed_identifier(value: &str, length: usize, name: &str) -> Result<()> {
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PersistenceError::InvalidInput(format!(
            "{name} must be a {length}-character hexadecimal identifier"
        )));
    }
    Ok(())
}

fn validate_prefixed_identifier(value: &str, prefix: &str, name: &str) -> Result<()> {
    if value.len() != prefix.len() + 32
        || !value.starts_with(prefix)
        || !value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(PersistenceError::InvalidInput(format!(
            "{name} has an invalid local identifier shape"
        )));
    }
    Ok(())
}

fn parse_stored_decimal(value: &str) -> Result<ExactDecimal> {
    let parsed = ExactDecimal::from_str(value).map_err(|error| {
        PersistenceError::Integrity(format!("invalid exact decimal in storage: {error}"))
    })?;
    if parsed.canonical() != value {
        return Err(PersistenceError::Integrity(
            "noncanonical exact decimal in storage".to_owned(),
        ));
    }
    Ok(parsed)
}

fn parse_intent(value: &str) -> Result<UserIntent> {
    match value {
        "buy-asset" => Ok(UserIntent::BuyAsset),
        "sell-asset" => Ok(UserIntent::SellAsset),
        _ => Err(PersistenceError::Integrity(
            "invalid stored user intent".to_owned(),
        )),
    }
}

fn load_payments(connection: &Connection, content_hash: &str) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(
            "SELECT payment_method FROM ad_version_payments
             WHERE content_hash = ?1 ORDER BY payment_method",
        )
        .map_err(PersistenceError::from)?;
    statement
        .query_map([content_hash], |row| row.get::<_, String>(0))
        .map_err(PersistenceError::from)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(PersistenceError::from)
}

fn bool_i64(value: bool) -> i64 {
    i64::from(u8::from(value))
}

fn u64_to_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| PersistenceError::InvalidInput(format!("{name} exceeds SQLite integer range")))
}

fn usize_to_i64(value: usize, name: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| PersistenceError::InvalidInput(format!("{name} exceeds SQLite integer range")))
}

fn connection_ref(inner: &StoreInner) -> Result<&Connection> {
    inner.connection.as_ref().ok_or_else(|| {
        PersistenceError::Sqlite("database connection is temporarily unavailable".to_owned())
    })
}

fn connection_mut(inner: &mut StoreInner) -> Result<&mut Connection> {
    inner.connection.as_mut().ok_or_else(|| {
        PersistenceError::Sqlite("database connection is temporarily unavailable".to_owned())
    })
}

fn close_guard_connection(inner: &mut StoreInner) -> Result<()> {
    let connection = inner.connection.take().ok_or_else(|| {
        PersistenceError::Sqlite("database connection is already closed".to_owned())
    })?;
    match connection.close() {
        Ok(()) => Ok(()),
        Err((connection, error)) => {
            inner.connection = Some(connection);
            Err(PersistenceError::from(error))
        }
    }
}

fn restore_rollback_database_path(database_path: &Path) -> PathBuf {
    database_path.with_extension("rollback.sqlite3")
}

fn restore_rollback_key_path(identity_key_path: &Path) -> PathBuf {
    identity_key_path.with_extension("rollback.key")
}

fn restore_staged_database_path(database_path: &Path) -> PathBuf {
    database_path.with_extension("restore.sqlite3")
}

fn restore_staged_key_path(identity_key_path: &Path) -> PathBuf {
    identity_key_path.with_extension("restore.key")
}

fn restore_marker_path(database_path: &Path) -> Result<PathBuf> {
    let directory = database_path.parent().ok_or_else(|| {
        PersistenceError::Integrity("database path has no parent directory".to_owned())
    })?;
    Ok(directory.join(RESTORE_MARKER_FILE_NAME))
}

fn create_restore_marker(path: &Path, restored_at_ms: i64) -> Result<()> {
    let mut marker = FileOpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    marker.write_all(restored_at_ms.to_string().as_bytes())?;
    marker.sync_all()?;
    sync_parent_directory(path)
}

fn complete_restore_marker(path: &Path) -> Result<()> {
    remove_if_exists(path)?;
    sync_parent_directory(path)
}

fn recover_interrupted_restore(database_path: &Path, identity_key_path: &Path) -> Result<()> {
    let marker = restore_marker_path(database_path)?;
    let rollback_database = restore_rollback_database_path(database_path);
    let rollback_key = restore_rollback_key_path(identity_key_path);
    let staged_database = restore_staged_database_path(database_path);
    let staged_key = restore_staged_key_path(identity_key_path);

    if marker.exists() {
        if rollback_key.exists() && !rollback_database.exists() {
            return Err(PersistenceError::Integrity(
                "interrupted restore has an inconsistent rollback inventory".to_owned(),
            ));
        }
        remove_sidecars(database_path)?;
        if rollback_database.exists() {
            remove_if_exists(database_path)?;
            fs::rename(&rollback_database, database_path)?;
        }
        if rollback_key.exists() {
            remove_if_exists(identity_key_path)?;
            fs::rename(&rollback_key, identity_key_path)?;
        }
        if !database_path.exists() || !identity_key_path.exists() {
            return Err(PersistenceError::Integrity(
                "interrupted restore cannot recover the prior database and identity key".to_owned(),
            ));
        }
        remove_if_exists(&staged_database)?;
        remove_if_exists(&staged_key)?;
        complete_restore_marker(&marker)?;
    } else {
        remove_if_exists(&rollback_database)?;
        remove_if_exists(&rollback_key)?;
        remove_if_exists(&staged_database)?;
        remove_if_exists(&staged_key)?;
    }
    Ok(())
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            PersistenceError::Integrity("managed path has no parent directory".to_owned())
        })?;
        fs::File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rollback_restore_swap(
    inner: &mut StoreInner,
    database_path: &Path,
    identity_key_path: &Path,
    rollback_database: &Path,
    rollback_key: &Path,
    staged_database: &Path,
    staged_key: &Path,
    restore_marker: &Path,
    busy_timeout_ms: u64,
    replacement_error: PersistenceError,
) -> Result<RestoreOutcome> {
    let rollback_result = (|| -> Result<(Connection, [u8; 32])> {
        if rollback_database.exists() {
            remove_if_exists(database_path)?;
            fs::rename(rollback_database, database_path)?;
        }
        if rollback_key.exists() {
            remove_if_exists(identity_key_path)?;
            fs::rename(rollback_key, identity_key_path)?;
        }
        remove_if_exists(staged_database)?;
        remove_if_exists(staged_key)?;
        let key = read_identity_key(identity_key_path)?;
        let connection = open_connection(database_path, busy_timeout_ms)?;
        check_integrity(&connection, true)?;
        complete_restore_marker(restore_marker)?;
        Ok((connection, key))
    })();
    match rollback_result {
        Ok((connection, key)) => {
            inner.connection = Some(connection);
            inner.identity_key = key;
            Err(PersistenceError::RestoreRolledBack(
                replacement_error.to_string(),
            ))
        }
        Err(rollback_error) => Err(PersistenceError::RestoreRollbackFailed(format!(
            "replacement: {replacement_error}; rollback: {rollback_error}"
        ))),
    }
}

fn remove_sidecars(database_path: &Path) -> Result<()> {
    remove_if_exists(&path_with_suffix(database_path, "-wal"))?;
    remove_if_exists(&path_with_suffix(database_path, "-shm"))
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.to_string_lossy()))
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn copy_synced(source: &Path, destination: &Path) -> Result<()> {
    remove_if_exists(destination)?;
    fs::copy(source, destination)?;
    FileOpenOptions::new()
        .write(true)
        .open(destination)?
        .sync_all()?;
    Ok(())
}

fn unique_automatic_backup_path(
    directory: &Path,
    created_at_ms: i64,
    old_schema_version: u32,
    prefix: &str,
) -> Result<PathBuf> {
    let base = automatic_backup_path(directory, created_at_ms, old_schema_version);
    if prefix == "migration-backup" && !base.exists() {
        return Ok(base);
    }
    let random = random_identifier("bk")?;
    Ok(directory.join(format!(
        "{prefix}-{created_at_ms:020}-schema-{old_schema_version}-{random}.zip"
    )))
}

fn clear_directory(directory: &Path) -> Result<u64> {
    fs::create_dir_all(directory)?;
    let mut deleted = 0_u64;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
        deleted = deleted.saturating_add(1);
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{Read, Write};

    use p2p_domain::{
        EligibilityFilters, MarketPair, MerchantFacts, PageReceiptTiming, PaymentMethod,
        RequestedAmount, ResultsTarget, SideQuality, Symbol, ValidatedAd, ValidatedAdInput,
    };
    use p2p_provider::{Acquisition, NormalizedAd, SideAcquisition};
    use serde_json::json;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipArchive, ZipWriter};

    use super::*;
    use crate::model::{RollupPeriod, SnapshotContext, SummaryInput};

    fn versions() -> RuntimeVersions {
        RuntimeVersions::current("0.1.0-test").expect("versions")
    }

    fn pair() -> MarketPair {
        MarketPair::new(
            Symbol::new("USDT").expect("asset"),
            Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair")
    }

    fn exact(value: &str) -> ExactDecimal {
        ExactDecimal::from_str(value).expect("decimal")
    }

    fn normalized_ad(intent: UserIntent, suffix: &str, observed_at_ms: i64) -> NormalizedAd {
        let payments = BTreeSet::from([
            PaymentMethod::new("BANK_A").expect("payment"),
            PaymentMethod::new("WALLET_B").expect("payment"),
        ]);
        let merchant = MerchantFacts::new(
            StableId::new(format!("provider-merchant-{suffix}")).expect("merchant ID"),
            250,
            exact("98.75"),
            exact("99.5"),
            true,
        )
        .expect("merchant");
        let ad = ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new(format!("provider-ad-{suffix}")).expect("ad ID"),
            advertiser_side: intent.expected_advertiser_side(),
            price: exact(if intent == UserIntent::BuyAsset {
                "50.125"
            } else {
                "50.875"
            }),
            min_fiat: exact("100"),
            max_fiat: exact("25000"),
            available_asset: exact("500.25"),
            payments,
            merchant,
            observed_at_ms,
        })
        .expect("ad");
        NormalizedAd {
            ad,
            public_nickname: None,
            merchant_active_seconds: 42,
        }
    }

    fn complete_acquisition(request_suffix: &str, observed_at_ms: i64) -> Acquisition {
        let quality = SideQuality::new(1, 1, 0, 0, 20, Some(1), true).expect("quality");
        Acquisition {
            request_id: StableId::new(format!("provider-request-{request_suffix}"))
                .expect("request"),
            pair: pair(),
            buy: SideAcquisition {
                ads: vec![normalized_ad(UserIntent::BuyAsset, "buy-1", observed_at_ms)],
                quality,
                rejection_counts: BTreeMap::new(),
            },
            sell: SideAcquisition {
                ads: vec![normalized_ad(
                    UserIntent::SellAsset,
                    "sell-1",
                    observed_at_ms,
                )],
                quality,
                rejection_counts: BTreeMap::new(),
            },
            page_receipts: vec![
                PageReceiptTiming::new(UserIntent::BuyAsset, 1, observed_at_ms + 1).expect("page"),
                PageReceiptTiming::new(UserIntent::SellAsset, 1, observed_at_ms + 2).expect("page"),
            ],
        }
    }

    fn context() -> SnapshotContext {
        SnapshotContext {
            pair: pair(),
            amount: RequestedAmount::new(exact("10000"), AmountMode::Fiat).expect("amount"),
            filters: EligibilityFilters::neutral(),
            result_target: ResultsTarget::new(20).expect("target"),
        }
    }

    fn summaries() -> Vec<SummaryInput> {
        vec![
            SummaryInput {
                intent: UserIntent::BuyAsset,
                metric_key: "eligible-count".to_owned(),
                value: Some(exact("1")),
                unit: "orders".to_owned(),
            },
            SummaryInput {
                intent: UserIntent::SellAsset,
                metric_key: "eligible-count".to_owned(),
                value: Some(exact("1")),
                unit: "orders".to_owned(),
            },
        ]
    }

    fn publish_at(
        store: &PersistenceStore,
        acquisition: &Acquisition,
        committed_ms: i64,
    ) -> PublicationOutcome {
        store
            .publish_complete_snapshot(PublicationInput {
                acquisition,
                context: context(),
                request_started_ms: committed_ms - 100,
                last_page_received_ms: committed_ms - 10,
                validated_ms: committed_ms - 5,
                committed_ms,
                agent_checked_ms: Some(committed_ms - 5),
                refresh_interval_seconds: 20,
                summaries: summaries(),
            })
            .expect("publish")
    }

    fn row_count(store: &PersistenceStore, table: &str) -> i64 {
        let guard = store.lock_inner().expect("lock");
        connection_ref(&guard)
            .expect("connection")
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count")
    }

    #[test]
    fn random_storage_identifiers_have_fixed_nonsecret_shape() {
        let id = random_identifier("sn").expect("identifier");
        assert_eq!(id.len(), 34);
        assert!(id.starts_with("sn"));
        assert!(id[2..].bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn opens_versioned_strict_wal_schema_without_real_or_forbidden_columns() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        assert_eq!(store.schema_version().expect("version"), 1);
        let guard = store.lock_inner().expect("lock");
        let connection = connection_ref(&guard).expect("connection");
        assert_eq!(
            connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .expect("foreign keys"),
            1
        );
        assert_eq!(
            connection
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .expect("journal")
                .to_ascii_lowercase(),
            "wal"
        );
        let schema: String = connection
            .query_row(
                "SELECT group_concat(sql, '\n') FROM sqlite_schema WHERE sql IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("schema");
        let lower = schema.to_ascii_lowercase();
        assert!(!lower.contains(" real"));
        assert!(!lower.contains("nickname"));
        assert!(!lower.contains("raw_response"));
        assert!(!lower.contains("raw_body"));
        validate_semantic_storage(connection).expect("semantic audit");
    }

    #[test]
    fn reopen_rejects_noncanonical_decimals_sensitive_settings_and_schema_drift() {
        let decimal_root = TempDir::new().expect("decimal root");
        let decimal_store =
            PersistenceStore::open(decimal_root.path(), versions(), 1).expect("open decimal");
        publish_at(&decimal_store, &complete_acquisition("decimal", 100), 150);
        let decimal_path = decimal_store.database_path.clone();
        drop(decimal_store);
        let decimal_connection = Connection::open(&decimal_path).expect("raw decimal connection");
        decimal_connection
            .execute("UPDATE contexts SET amount_text = '01.0'", [])
            .expect("tamper decimal");
        drop(decimal_connection);
        let decimal_error = PersistenceStore::open(decimal_root.path(), versions(), 2)
            .err()
            .expect("noncanonical decimal must fail");
        assert!(
            matches!(decimal_error, PersistenceError::Integrity(_)),
            "{decimal_error:?}"
        );

        let setting_root = TempDir::new().expect("setting root");
        let setting_store =
            PersistenceStore::open(setting_root.path(), versions(), 1).expect("open setting");
        let setting_path = setting_store.database_path.clone();
        drop(setting_store);
        let setting_connection = Connection::open(&setting_path).expect("raw setting connection");
        setting_connection
            .execute(
                "INSERT INTO settings(section_key, setting_key, value_json, updated_at_ms)
                 VALUES ('source', 'advanced', '{\"nested\":{\"privateKey\":\"no\"}}', 1)",
                [],
            )
            .expect("tamper setting");
        drop(setting_connection);
        let setting_error = PersistenceStore::open(setting_root.path(), versions(), 2)
            .err()
            .expect("sensitive setting must fail");
        assert!(
            matches!(setting_error, PersistenceError::Integrity(_)),
            "{setting_error:?}"
        );

        let schema_root = TempDir::new().expect("schema root");
        let schema_store =
            PersistenceStore::open(schema_root.path(), versions(), 1).expect("open schema");
        let schema_path = schema_store.database_path.clone();
        drop(schema_store);
        let schema_connection = Connection::open(&schema_path).expect("raw schema connection");
        schema_connection
            .execute_batch("CREATE TABLE unexpected_local_data(value TEXT) STRICT;")
            .expect("tamper schema");
        drop(schema_connection);
        let schema_error = PersistenceStore::open(schema_root.path(), versions(), 2)
            .err()
            .expect("schema drift must fail");
        assert!(
            matches!(schema_error, PersistenceError::Integrity(_)),
            "{schema_error:?}"
        );

        let json_root = TempDir::new().expect("json root");
        let json_store =
            PersistenceStore::open(json_root.path(), versions(), 1).expect("open json");
        let json_path = json_store.database_path.clone();
        drop(json_store);
        let json_connection = Connection::open(&json_path).expect("raw json connection");
        json_connection
            .execute(
                "INSERT INTO pair_catalog(
                    pair_key, asset, fiat, enabled, disabled_reason, verified_at_ms,
                    disabled_at_ms, provider_adapter_version, precision_json
                 ) VALUES ('USDT/EGP', 'USDT', 'EGP', 1, NULL, 1, NULL, 'adapter', '{')",
                [],
            )
            .expect("tamper json");
        drop(json_connection);
        let json_error = PersistenceStore::open(json_root.path(), versions(), 2)
            .err()
            .expect("malformed JSON must fail");
        assert!(
            matches!(json_error, PersistenceError::Integrity(_)),
            "{json_error:?}"
        );
    }

    #[test]
    fn publishes_complete_two_side_snapshot_atomically_and_deduplicates_content() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        let first_acquisition = complete_acquisition("one", 100);
        let first = publish_at(&store, &first_acquisition, 150);
        assert_eq!(first.inserted_ad_versions, 2);
        assert_eq!(first.reused_ad_versions, 0);
        assert_eq!(row_count(&store, "snapshots"), 1);
        assert_eq!(row_count(&store, "snapshot_ad_membership"), 2);
        assert_eq!(row_count(&store, "ad_versions"), 2);

        let replay = store.load_snapshot(&first.snapshot_id).expect("replay");
        assert_eq!(replay.ads.len(), 2);
        assert_eq!(replay.amount.canonical(), "10000");
        assert!(replay.ads.iter().all(|ad| {
            ad.ad_key.len() == 64
                && ad.merchant_key.len() == 64
                && !ad.ad_key.contains("provider")
                && !ad.merchant_key.contains("provider")
        }));

        let second_acquisition = complete_acquisition("two", 200);
        let second = publish_at(&store, &second_acquisition, 250);
        assert_eq!(second.inserted_ad_versions, 0);
        assert_eq!(second.reused_ad_versions, 2);
        assert_eq!(row_count(&store, "snapshots"), 2);
        assert_eq!(row_count(&store, "ad_versions"), 2);
        checkpoint(connection_ref(&store.lock_inner().expect("lock")).expect("connection"))
            .expect("checkpoint");
        let bytes = fs::read(store.database_path()).expect("database bytes");
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("provider-ad-buy-1"));
        assert!(!text.contains("provider-merchant-buy-1"));
        assert!(!text.contains("provider-request-one"));
    }

    #[test]
    fn incomplete_input_and_faults_never_leave_partial_snapshots() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        let mut incomplete = complete_acquisition("incomplete", 100);
        incomplete.buy.quality =
            SideQuality::new(1, 1, 0, 0, 20, Some(20), false).expect("quality");
        let result = store.publish_complete_snapshot(PublicationInput {
            acquisition: &incomplete,
            context: context(),
            request_started_ms: 90,
            last_page_received_ms: 102,
            validated_ms: 110,
            committed_ms: 120,
            agent_checked_ms: None,
            refresh_interval_seconds: 20,
            summaries: summaries(),
        });
        assert!(matches!(result, Err(PersistenceError::NotPublishable(_))));
        assert_eq!(row_count(&store, "snapshots"), 0);

        for fault in [
            FaultPoint::PublicationAfterHeader,
            FaultPoint::PublicationAfterAds,
        ] {
            let acquisition = complete_acquisition(&format!("fault-{fault:?}"), 200);
            let result = store.publish_with_fault(
                PublicationInput {
                    acquisition: &acquisition,
                    context: context(),
                    request_started_ms: 190,
                    last_page_received_ms: 202,
                    validated_ms: 210,
                    committed_ms: 220,
                    agent_checked_ms: None,
                    refresh_interval_seconds: 20,
                    summaries: summaries(),
                },
                fault,
            );
            assert!(matches!(result, Err(PersistenceError::FaultInjected)));
            assert_eq!(row_count(&store, "snapshots"), 0);
            assert_eq!(row_count(&store, "ad_versions"), 0);
        }
    }

    #[test]
    fn abrupt_process_death_rolls_back_uncommitted_transaction() {
        const CRASH_ROOT: &str = "P2P_DESK_CRASH_TEST_ROOT";
        if let Ok(root) = std::env::var(CRASH_ROOT) {
            let store = PersistenceStore::open(root, versions(), 1).expect("child open");
            let mut guard = store.lock_inner().expect("child lock");
            let connection = connection_mut(&mut guard).expect("child connection");
            connection.execute_batch("BEGIN IMMEDIATE;").expect("begin");
            connection
                .execute(
                    "INSERT INTO settings(section_key, setting_key, value_json, updated_at_ms)
                     VALUES ('crash-test', 'uncommitted', 'true', 1)",
                    [],
                )
                .expect("uncommitted insert");
            std::process::exit(91);
        }

        let root = TempDir::new().expect("root");
        drop(PersistenceStore::open(root.path(), versions(), 1).expect("initial open"));
        let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "store::tests::abrupt_process_death_rolls_back_uncommitted_transaction",
                "--nocapture",
            ])
            .env(CRASH_ROOT, root.path())
            .status()
            .expect("launch crash child");
        assert!(!status.success(), "child must terminate abruptly");

        let reopened = PersistenceStore::open(root.path(), versions(), 2).expect("recover");
        assert_eq!(
            reopened
                .load_setting("crash-test", "uncommitted")
                .expect("load"),
            None
        );
        reopened.integrity_check().expect("integrity after crash");
    }

    #[test]
    fn migration_fault_rolls_back_and_reopen_creates_validated_pre_migration_backup() {
        let root = TempDir::new().expect("root");
        let result = PersistenceStore::open_with_options(
            root.path(),
            versions(),
            10,
            StoreOptions {
                fault: FaultPoint::MigrationBeforeCommit,
                ..StoreOptions::default()
            },
        );
        assert!(matches!(result, Err(PersistenceError::FaultInjected)));
        let database = root
            .path()
            .join(DATABASE_DIRECTORY)
            .join(DATABASE_FILE_NAME);
        let connection = Connection::open(&database).expect("open failed migration db");
        assert_eq!(schema_version(&connection).expect("schema"), 0);
        let table_exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='snapshots'",
                [],
                |row| row.get(0),
            )
            .expect("table count");
        assert_eq!(table_exists, 0);
        drop(connection);

        let store = PersistenceStore::open(root.path(), versions(), 20).expect("reopen");
        assert_eq!(store.schema_version().expect("version"), 1);
        let backups = fs::read_dir(root.path().join(AUTOMATIC_BACKUP_DIRECTORY))
            .expect("backup dir")
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|value| value == "zip"))
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn backup_restore_round_trip_validates_hashes_and_rolls_back_injected_failure() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        store
            .save_setting("refresh", "seconds", &json!(20), 10)
            .expect("setting");
        let backup_path = root.path().join("manual-backup.zip");
        let backup = store.create_backup(&backup_path, 20).expect("backup");
        assert_eq!(backup.manifest.database_schema_version, 1);
        assert_eq!(backup.manifest.entries.len(), 2);
        store
            .save_setting("refresh", "seconds", &json!(99), 30)
            .expect("changed setting");
        let restored = store.restore_backup(&backup_path, 40).expect("restore");
        assert_eq!(restored.restored_schema_version, 1);
        assert_eq!(
            store.load_setting("refresh", "seconds").expect("load"),
            Some(json!(20))
        );

        for fault in [
            FaultPoint::RestoreAfterDatabaseMoved,
            FaultPoint::RestoreAfterKeyMoved,
            FaultPoint::RestoreAfterDatabaseInstalled,
            FaultPoint::RestoreAfterReplacement,
        ] {
            let rollback_root = TempDir::new().expect("rollback root");
            let rollback_store = PersistenceStore::open_with_options(
                rollback_root.path(),
                versions(),
                1,
                StoreOptions {
                    fault,
                    ..StoreOptions::default()
                },
            )
            .expect("rollback store");
            rollback_store
                .save_setting("refresh", "seconds", &json!(20), 10)
                .expect("initial");
            let rollback_backup = rollback_root.path().join("rollback-source.zip");
            rollback_store
                .create_backup(&rollback_backup, 20)
                .expect("backup");
            rollback_store
                .save_setting("refresh", "seconds", &json!(77), 30)
                .expect("changed");
            let result = rollback_store.restore_backup(&rollback_backup, 40);
            assert!(matches!(
                result,
                Err(PersistenceError::RestoreRolledBack(_))
            ));
            assert_eq!(
                rollback_store
                    .load_setting("refresh", "seconds")
                    .expect("load after rollback"),
                Some(json!(77))
            );
            rollback_store
                .integrity_check()
                .expect("rollback integrity");
        }
    }

    #[test]
    fn startup_recovers_an_interrupted_restore_swap() {
        let root = TempDir::new().expect("restore crash root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        store
            .save_setting("refresh", "seconds", &json!(77), 10)
            .expect("setting");
        let database_path = store.database_path.clone();
        let identity_key_path = store.identity_key_path.clone();
        drop(store);

        let rollback_database = restore_rollback_database_path(&database_path);
        let rollback_key = restore_rollback_key_path(&identity_key_path);
        let staged_database = restore_staged_database_path(&database_path);
        let staged_key = restore_staged_key_path(&identity_key_path);
        let marker = restore_marker_path(&database_path).expect("marker path");
        create_restore_marker(&marker, 20).expect("marker");
        fs::rename(&database_path, &rollback_database).expect("move database");
        fs::rename(&identity_key_path, &rollback_key).expect("move key");
        fs::write(&database_path, b"partial replacement").expect("partial database");
        fs::write(&identity_key_path, [0_u8; 32]).expect("partial key");
        fs::write(&staged_database, b"staged database").expect("staged database");
        fs::write(&staged_key, b"staged key").expect("staged key");

        let recovered = PersistenceStore::open(root.path(), versions(), 30).expect("recover");
        assert_eq!(
            recovered
                .load_setting("refresh", "seconds")
                .expect("load recovered setting"),
            Some(json!(77))
        );
        recovered.integrity_check().expect("recovered integrity");
        assert!(!marker.exists());
        assert!(!rollback_database.exists());
        assert!(!rollback_key.exists());
        assert!(!staged_database.exists());
        assert!(!staged_key.exists());
    }

    #[test]
    fn automatic_backup_retention_is_combined_and_timestamp_ordered() {
        let root = TempDir::new().expect("backup retention root");
        for timestamp in 1..=8_i64 {
            let prefix = if timestamp % 2 == 0 {
                "restore-safety"
            } else {
                "migration-backup"
            };
            let path = root.path().join(format!(
                "{prefix}-{timestamp:020}-schema-1-bk00000000000000000000000000000000.zip"
            ));
            fs::write(path, timestamp.to_string()).expect("backup marker");
        }
        retain_latest_automatic_backups(root.path()).expect("retain backups");
        let mut timestamps = fs::read_dir(root.path())
            .expect("backup directory")
            .map(|entry| {
                let name = entry.expect("entry").file_name();
                let name = name.to_str().expect("name");
                name.split('-')
                    .find_map(|segment| (segment.len() == 20).then(|| segment.parse::<i64>().ok()))
                    .flatten()
                    .expect("timestamp")
            })
            .collect::<Vec<_>>();
        timestamps.sort_unstable();
        assert_eq!(timestamps, vec![4, 5, 6, 7, 8]);
    }

    #[test]
    fn backup_tampering_incompatibility_and_free_space_fail_before_restore() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        let backup_path = root.path().join("source.zip");
        store.create_backup(&backup_path, 2).expect("backup");
        let tampered = root.path().join("tampered.zip");
        rewrite_archive(&backup_path, &tampered, |name, bytes| {
            if name == "database.sqlite3" {
                bytes.push(0);
            }
        });
        assert!(matches!(
            store.restore_backup(&tampered, 3),
            Err(PersistenceError::InvalidBackup(_))
        ));

        let incompatible = root.path().join("incompatible.zip");
        rewrite_archive(&backup_path, &incompatible, |name, bytes| {
            if name == "manifest.json" {
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(bytes).expect("manifest");
                manifest["databaseSchemaVersion"] = json!(DATABASE_SCHEMA_VERSION + 1);
                *bytes = serde_json::to_vec_pretty(&manifest).expect("manifest json");
            }
        });
        assert!(matches!(
            store.restore_backup(&incompatible, 4),
            Err(PersistenceError::Incompatible(_))
        ));

        let low_space_root = TempDir::new().expect("low space root");
        let low_space_store = PersistenceStore::open_with_options(
            low_space_root.path(),
            versions(),
            1,
            StoreOptions {
                available_space_override: Some(0),
                ..StoreOptions::default()
            },
        )
        .expect("low space store");
        assert!(matches!(
            low_space_store.restore_backup(&backup_path, 5),
            Err(PersistenceError::InvalidBackup(_))
        ));
    }

    #[test]
    fn retention_prunes_complete_boundaries_in_tier_order_and_preserves_foundations() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        store
            .save_setting("history", "enabled", &json!(true), 1)
            .expect("setting");
        let old_acquisition = complete_acquisition("old", 80);
        let old = publish_at(&store, &old_acquisition, 100);
        let new_acquisition = complete_acquisition("new", 180);
        let new = publish_at(&store, &new_acquisition, 200);
        store
            .insert_rollup(RollupInput {
                pair: pair(),
                intent: UserIntent::BuyAsset,
                period: RollupPeriod::Hour,
                period_start_ms: 50,
                metric_key: "median-price".to_owned(),
                value: Some(exact("50")),
                unit: "EGP/USDT".to_owned(),
                sample_count: 2,
            })
            .expect("rollup");
        let first = store
            .prune(200, RetentionPolicy::for_test(50, 500, 1_000, u64::MAX))
            .expect("detail prune");
        assert_eq!(first.detail_snapshots_pruned, 1);
        assert!(
            store
                .load_snapshot(&old.snapshot_id)
                .expect("old header")
                .ads
                .is_empty()
        );
        assert_eq!(
            store
                .load_snapshot(&new.snapshot_id)
                .expect("new")
                .ads
                .len(),
            2
        );
        assert_eq!(row_count(&store, "settings"), 1);

        let second = store
            .prune(1_500, RetentionPolicy::for_test(50, 500, 1_000, u64::MAX))
            .expect("summary and rollup prune");
        assert_eq!(second.summary_snapshots_pruned, 2);
        assert_eq!(second.rollups_pruned, 1);
        assert_eq!(row_count(&store, "snapshots"), 0);
        assert_eq!(row_count(&store, "settings"), 1);
    }

    #[test]
    fn managed_cap_prunes_oldest_detail_at_whole_snapshot_boundaries_and_keeps_latest() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        let first_acquisition = complete_acquisition("cap-one", 10);
        let first = publish_at(&store, &first_acquisition, 100);
        let second_acquisition = complete_acquisition("cap-two", 110);
        let second = publish_at(&store, &second_acquisition, 200);
        let latest_acquisition = complete_acquisition("cap-three", 210);
        let latest = publish_at(&store, &latest_acquisition, 300);
        let outcome = store
            .prune(300, RetentionPolicy::for_test(10_000, 20_000, 30_000, 1))
            .expect("cap prune");
        assert_eq!(outcome.cap_detail_snapshots_pruned, 2);
        assert!(
            store
                .load_snapshot(&first.snapshot_id)
                .expect("first")
                .ads
                .is_empty()
        );
        assert!(
            store
                .load_snapshot(&second.snapshot_id)
                .expect("second")
                .ads
                .is_empty()
        );
        assert_eq!(
            store
                .load_snapshot(&latest.snapshot_id)
                .expect("latest")
                .ads
                .len(),
            2
        );
        assert_eq!(row_count(&store, "snapshots"), 3);
        assert_eq!(row_count(&store, "retention_events"), 1);
        assert!(
            !outcome.cap_satisfied,
            "schema itself exceeds a one-byte test cap"
        );
    }

    #[test]
    fn cost_versions_preserve_unknown_distinct_from_explicit_zero() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        let base = CostProfileInput {
            pair: pair(),
            route_key: "BANK_A-to-WALLET_B".to_owned(),
            leg: UserIntent::BuyAsset,
            payment_method: PaymentMethod::new("BANK_A").expect("payment"),
            label: "Bank transfer cost".to_owned(),
            effective_from_ms: 100,
            effective_to_ms: None,
            fixed_fiat: None,
            percent_fiat: None,
            fixed_asset: None,
            minimum_charge: None,
            maximum_charge: None,
            fixed_buffer: None,
            percent_buffer: None,
            source_label: None,
            note: None,
            created_at_ms: 100,
        };
        let unknown = store
            .create_cost_profile_version(base.clone())
            .expect("unknown version");
        let mut zero_input = base.clone();
        zero_input.effective_from_ms = 200;
        zero_input.created_at_ms = 200;
        zero_input.fixed_fiat = Some(ExactDecimal::ZERO);
        let zero = store
            .create_cost_profile_version(zero_input)
            .expect("zero version");
        assert_eq!(unknown.profile_id, zero.profile_id);
        assert_ne!(unknown.version_id, zero.version_id);

        let mut other_payment = base;
        other_payment.payment_method = PaymentMethod::new("BANK_B").expect("other payment");
        let distinct = store
            .create_cost_profile_version(other_payment)
            .expect("distinct payment profile");
        assert_ne!(unknown.profile_id, distinct.profile_id);
        assert_eq!(row_count(&store, "cost_profiles"), 2);

        let guard = store.lock_inner().expect("lock");
        let values = connection_ref(&guard)
            .expect("connection")
            .prepare(
                "SELECT fixed_fiat_text FROM cost_profile_versions
                 WHERE profile_id = ?1 ORDER BY effective_from_ms",
            )
            .expect("prepare")
            .query_map([unknown.profile_id], |row| row.get::<_, Option<String>>(0))
            .expect("query")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("rows");
        assert_eq!(values, vec![None, Some("0".to_owned())]);
    }

    #[test]
    fn settings_catalog_annotations_views_and_clear_scopes_are_independent() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open(root.path(), versions(), 1).expect("open");
        store
            .save_setting("refresh", "seconds", &json!(20), 1)
            .expect("setting");
        assert!(matches!(
            store.save_setting("source", "api-key", &json!("forbidden"), 1),
            Err(PersistenceError::InvalidInput(_))
        ));
        assert!(matches!(
            store.save_setting("source", "session_token", &json!("forbidden"), 1),
            Err(PersistenceError::InvalidInput(_))
        ));
        assert!(matches!(
            store.save_setting(
                "source",
                "advanced",
                &json!({"nested": {"accessToken": "forbidden"}}),
                1,
            ),
            Err(PersistenceError::InvalidInput(_))
        ));
        store
            .save_catalog_pair(CatalogPairInput {
                pair: pair(),
                enabled: true,
                disabled_reason: None,
                verified_at_ms: 1,
                disabled_at_ms: None,
                precision: json!({"fiatScale": 2, "assetScale": 8}),
                payment_methods: BTreeSet::from([PaymentMethod::new("BANK_A").expect("payment")]),
            })
            .expect("catalog");
        store
            .save_annotation(AnnotationInput {
                chart_key: "history.price".to_owned(),
                context_hash: None,
                payload: json!({"type": "line", "x": "100"}),
                schema_version: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("annotation");
        store
            .save_named_view(NamedViewInput {
                chart_key: "history.price".to_owned(),
                name: "Review".to_owned(),
                context_hash: None,
                payload: json!({"range": "24h"}),
                schema_version: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
            })
            .expect("view");
        let log_path = root.path().join(LOG_DIRECTORY).join("app.log");
        fs::write(&log_path, b"safe aggregate log").expect("log");

        store
            .clear(ClearScope::AnnotationsAndViews)
            .expect("clear documents");
        assert_eq!(row_count(&store, "chart_annotations"), 0);
        assert_eq!(row_count(&store, "named_views"), 0);
        assert_eq!(row_count(&store, "settings"), 1);
        assert_eq!(row_count(&store, "pair_catalog"), 1);
        assert!(log_path.exists());

        store.clear(ClearScope::Logs).expect("clear logs");
        assert!(!log_path.exists());
        assert_eq!(row_count(&store, "settings"), 1);
        store.clear(ClearScope::Settings).expect("clear settings");
        assert_eq!(row_count(&store, "settings"), 0);
        assert_eq!(row_count(&store, "pair_catalog"), 1);
    }

    #[test]
    fn busy_disk_full_and_corruption_are_explicit_fail_closed_errors() {
        let root = TempDir::new().expect("root");
        let store = PersistenceStore::open_with_options(
            root.path(),
            versions(),
            1,
            StoreOptions {
                busy_timeout_ms: 10,
                ..StoreOptions::default()
            },
        )
        .expect("open");
        let locker = Connection::open(store.database_path()).expect("locker");
        locker.execute_batch("BEGIN IMMEDIATE;").expect("lock");
        assert!(matches!(
            store.save_setting("refresh", "seconds", &json!(20), 2),
            Err(PersistenceError::Busy)
        ));
        locker.execute_batch("ROLLBACK;").expect("unlock");

        let mut guard = store.lock_inner().expect("lock");
        let connection = connection_mut(&mut guard).expect("connection");
        let page_count: i64 = connection
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .expect("page count");
        connection
            .execute_batch(&format!("PRAGMA max_page_count = {page_count};"))
            .expect("max pages");
        let large = "x".repeat(250_000);
        let mut full = false;
        for index in 0..100 {
            let result = connection.execute(
                "INSERT INTO settings(section_key, setting_key, value_json, updated_at_ms)
                 VALUES ('disk-test', ?1, ?2, 1)",
                params![format!("key-{index}"), large],
            );
            if let Err(error) = result {
                full = matches!(PersistenceError::from(error), PersistenceError::DiskFull);
                break;
            }
        }
        assert!(full, "SQLite max_page_count must surface DatabaseFull");
        drop(guard);

        let corrupt_root = TempDir::new().expect("corrupt root");
        let database_dir = corrupt_root.path().join(DATABASE_DIRECTORY);
        fs::create_dir_all(&database_dir).expect("database dir");
        fs::write(
            database_dir.join(DATABASE_FILE_NAME),
            b"not a SQLite database",
        )
        .expect("corrupt db");
        fs::write(database_dir.join(IDENTITY_KEY_FILE_NAME), [7_u8; 32]).expect("key");
        let error = PersistenceStore::open(corrupt_root.path(), versions(), 1)
            .err()
            .expect("corrupt database must fail");
        assert!(matches!(error, PersistenceError::Corrupt), "{error:?}");
    }

    fn rewrite_archive(
        source: &Path,
        destination: &Path,
        mut edit: impl FnMut(&str, &mut Vec<u8>),
    ) {
        let source_file = FileOpenOptions::new()
            .read(true)
            .open(source)
            .expect("source zip");
        let mut archive = ZipArchive::new(source_file).expect("archive");
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("entry");
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read entry");
            edit(&name, &mut bytes);
            entries.push((name, bytes));
        }
        let output = FileOpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .expect("destination zip");
        let mut writer = ZipWriter::new(output);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in entries {
            writer.start_file(name, options).expect("start entry");
            writer.write_all(&bytes).expect("write entry");
        }
        writer.finish().expect("finish archive");
    }
}
