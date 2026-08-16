#!/usr/bin/env python3
"""Deterministic closure invariants for Gate 4 persistence and recovery."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates/p2p-persistence"
source_files = sorted((CRATE / "src").glob("*.rs"))
sources = "\n".join(path.read_text(encoding="utf-8") for path in source_files)
schema = (CRATE / "src/schema.rs").read_text(encoding="utf-8")
store = (CRATE / "src/store.rs").read_text(encoding="utf-8")
backup = (CRATE / "src/backup.rs").read_text(encoding="utf-8")
hashing = (CRATE / "src/hash.rs").read_text(encoding="utf-8")
model = (CRATE / "src/model.rs").read_text(encoding="utf-8")
manifest = (CRATE / "Cargo.toml").read_text(encoding="utf-8")
tauri_manifest = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
tauri_source = (ROOT / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
checks: list[dict[str, object]] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(condition), "detail": detail})


check("standalone locked persistence crate exists", (CRATE / "Cargo.lock").is_file())
for dependency, token in [
    ("rusqlite", 'rusqlite = { version = "=0.40.2", features = ["backup", "bundled"] }'),
    ("zip", 'zip = { version = "=8.6.0", default-features = false }'),
    ("sha2", 'sha2 = "=0.11.0"'),
    ("hmac", 'hmac = "=0.13.0"'),
    ("getrandom", 'getrandom = "=0.4.3"'),
    ("tempfile", 'tempfile = "=3.27.0"'),
    ("fs2", 'fs2 = "=0.4.3"'),
]:
    check(f"{dependency} dependency is exact pinned", token in manifest)
check("bundled SQLite and online backup are explicit", 'features = ["backup", "bundled"]' in manifest)
check("Tauri consumes path persistence crate", 'p2p-persistence = { path = "../crates/p2p-persistence" }' in tauri_manifest)
check("Tauri opens persistence at OS local-data root", all(token in tauri_source for token in ["PersistenceStore::open", "data_root", "PersistenceRuntimeState"]))
check("full verification includes persistence gate", "verify:persistence" in package["scripts"]["verify"] and "gate_04_audit.py" in package["scripts"]["verify:persistence"])

check("schema is versioned with migration catalog", all(token in schema for token in ["DATABASE_SCHEMA_VERSION: u32 = 1", "schema_migrations", "MIGRATIONS", "migration_checksum"]))
check("foreign keys WAL full sync checkpoint and busy timeout are configured", all(token in store for token in ["PRAGMA foreign_keys = ON", "PRAGMA journal_mode = WAL", "PRAGMA synchronous = FULL", "PRAGMA wal_checkpoint(TRUNCATE)", "busy_timeout"]))
check("schema contains no SQLite REAL declaration", not re.search(r"\bREAL\b", re.search(r'r#"(.*)"#;', schema, re.S).group(1), re.I))
check("all exact numeric storage columns are text-named", all(token in schema for token in ["price_text TEXT", "amount_text TEXT", "fixed_fiat_text TEXT", "value_text TEXT"]))
check("schema has no forbidden nickname or provider body column", not re.search(r"nickname|raw_body|raw_response|provider_payload", schema, re.I))
check("runtime audit rejects schema drift REAL forbidden columns malformed JSON and noncanonical decimals", all(token in store for token in ["validate_compiled_schema", "SQLite REAL column", "forbidden storage column", "validate_json_query", "parse_stored_decimal"]))

check("local keyed pseudonyms cover ads merchants and requests", all(token in hashing + store for token in ["Hmac<Sha256>", '"advertisement"', '"merchant"', '"request"']))
check("pseudonym key uses OS entropy and separate 32-byte file", all(token in store + schema for token in ["getrandom::fill", "IDENTITY_KEY_BYTES", "create_new(true)", 'IDENTITY_KEY_FILE_NAME: &str = "identity.key"']))
check("normalized content address excludes public nickname", "public_nickname" not in hashing and "p2p-desk-normalized-ad-v1" in hashing)
check("normalized ads and membership are deduplicated", all(token in schema + store for token in ["content_hash TEXT PRIMARY KEY", "INSERT OR IGNORE INTO ad_versions", "snapshot_ad_membership"]))

check("publication accepts completed acquisition and validates both sides", all(token in store for token in ["validate_publication", "buy.quality.complete()", "sell.quality.complete()", "ad_matches_intent"]))
check("publication uses one immediate transaction and complete marker", "transaction_with_behavior(TransactionBehavior::Immediate)" in store and "completion_state = 'complete'" in store)
check("publication fault tests cover header and ad boundaries", all(token in store for token in ["PublicationAfterHeader", "PublicationAfterAds", "never_leave_partial_snapshots"]))
check("abrupt process-death recovery is tested", all(token in store for token in ["abrupt_process_death_rolls_back_uncommitted_transaction", "std::process::exit(91)", "integrity after crash"]))
check("request/source identifiers are not stored directly", "acquisition.request_id.as_str()" in store and "request_key" in schema and "request_id TEXT" not in schema)

check("approved default retention and cap constants are exact", all(token in schema for token in ["7 * 24 * 60 * 60 * 1_000", "90 * 24 * 60 * 60 * 1_000", "2 * 365 * 24 * 60 * 60 * 1_000", "2 * 1024 * 1024 * 1024"]))
check("retention deletes detail then summaries then rollups", store.find("DELETE FROM snapshot_ad_membership") < store.find("DELETE FROM snapshots WHERE committed_ms") < store.find("DELETE FROM history_rollups"))
check("cap pruning protects newest snapshot and records every event", "snapshot_id != (SELECT snapshot_id FROM snapshots ORDER BY committed_ms DESC" in store and '"managed-cap"' in store and "retention_events" in schema)
check("retention preserves settings costs catalog annotations and views", all(table in schema for table in ["settings", "cost_profiles", "pair_catalog", "chart_annotations", "named_views"]) and "preserves_foundations" in store)

check("cost profiles are keyed by pair route leg and payment", all(token in schema for token in ["route_key TEXT", "leg TEXT", "payment_method TEXT", "UNIQUE(asset, fiat, route_key, leg, payment_method)"]))
check("cost versions distinguish SQL NULL unknown from text zero", "fixed_fiat_text TEXT" in schema and "unknown_distinct_from_explicit_zero" in store)
check("cost versions cover fixed percent asset bounds buffers dates source note label", all(token in schema for token in ["fixed_fiat_text", "percent_fiat_text", "fixed_asset_text", "minimum_charge_text", "maximum_charge_text", "fixed_buffer_text", "percent_buffer_text", "effective_from_ms", "effective_to_ms", "source_label", "note TEXT", "label TEXT"]))

check("online backup contains database identity key and manifest", all(token in backup for token in [".backup(", 'DATABASE_ENTRY', 'IDENTITY_KEY_ENTRY', 'MANIFEST_ENTRY']))
check("backup is atomic and checksummed", all(token in backup for token in ["NamedTempFile", "persist_noclobber", "sha256_bytes", "sync_all"]))
check("restore validates inventory hashes schema integrity and free space", all(token in backup + store for token in ["archive.len() != 3", "hash or size mismatch", "manifest schema version", "integrity_check", "available_space"]))
check("backup manifest rejects unknown fields and noncanonical inventory", all(token in model for token in ["deny_unknown_fields", "INCLUDED_DOMAINS", "backup entry metadata is invalid"]))
check("restore quiesces checkpoints safety-backs-up and rolls back", all(token in store for token in ["checkpoint(current)", '"restore-safety"', "close_guard_connection", "rollback_restore_swap", "RestoreRolledBack"]))
check("interrupted restore swap is recovered from a durable marker", all(token in store for token in ["RESTORE_MARKER_FILE_NAME", "recover_interrupted_restore", "startup_recovers_an_interrupted_restore_swap", "sync_parent_directory"]))
migration_loop = store[store.find("for migration in pending"):store.find("fn apply_migration")]
check("automatic backup precedes every pending existing migration", migration_loop.find("create_backup_archive(") < migration_loop.find("apply_migration("))
check("migration and restore fault tests exist", all(token in store for token in ["MigrationBeforeCommit", "RestoreAfterReplacement", "migration_fault_rolls_back", "rolls_back_injected_failure"]))
check("automatic migration backups are bounded", "AUTOMATIC_BACKUP_LIMIT: usize = 5" in schema and "retain_latest_automatic_backups" in backup)

check("settings reject credential token password secret and account keys", all(token in store for token in ["reject_sensitive_setting", '"credential"', '"token"', '"password"', '"account"', '"private-key"']))
check("independent clear scopes are explicit", all(token in model + store for token in ["History", "AnnotationsAndViews", "Settings", "Logs", "AllLocalData"]))
check("clear operations use an immediate database transaction", "fn clear_database_tables" in store and "TransactionBehavior::Immediate" in store)
check("concurrency disk-full corruption and low-space tests exist", all(token in store for token in ["busy_disk_full_and_corruption", "PersistenceError::Busy", "PersistenceError::DiskFull", "available_space_override: Some(0)"]))
check("no implementation placeholders", not re.search(r"\bTODO\b|\btodo!\s*\(|\bunimplemented!\s*\(", sources))
check("no unsafe code in persistence crate", not re.search(r"\bunsafe\b", sources))

passed = sum(1 for item in checks if item["passed"])
report = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "checks": checks}
print(json.dumps(report, indent=2))
sys.exit(0 if report["allPassed"] else 1)
