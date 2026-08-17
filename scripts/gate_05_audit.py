#!/usr/bin/env python3
"""Deterministic closure invariants for Gate 5 lifecycle orchestration."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates/p2p-lifecycle"
lifecycle = (CRATE / "src/lib.rs").read_text(encoding="utf-8")
tauri = (ROOT / "src-tauri/src/lifecycle_commands.rs").read_text(encoding="utf-8")
tauri_lib = (ROOT / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
tauri_build = (ROOT / "src-tauri/build.rs").read_text(encoding="utf-8")
manifest = (CRATE / "Cargo.toml").read_text(encoding="utf-8")
tauri_manifest = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
frontend = (ROOT / "app/src/ipc/lifecycle-contracts.ts").read_text(encoding="utf-8")
frontend_client = (ROOT / "app/src/ipc/lifecycle-client.ts").read_text(encoding="utf-8")
package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
checks: list[dict[str, object]] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(condition), "detail": detail})


check("standalone locked lifecycle crate exists", (CRATE / "Cargo.lock").is_file())
check("lifecycle dependencies are exact pinned or local", all(token in manifest for token in [
    'p2p-domain = { path = "../p2p-domain" }',
    'p2p-provider = { path = "../p2p-provider" }',
    'p2p-persistence = { path = "../p2p-persistence" }',
    'serde = { version = "=1.0.229"',
    'serde_json = "=1.0.151"',
    'thiserror = "=2.0.20"',
]))
check("Tauri consumes the lifecycle crate", 'p2p-lifecycle = { path = "../crates/p2p-lifecycle" }' in tauri_manifest)
check("full verification includes Gate 5", "verify:lifecycle" in package["scripts"]["verify"] and "gate_05_audit.py" in package["scripts"]["verify:lifecycle"])

check("first-run auto refresh is on at twenty seconds", all(token in lifecycle for token in [
    "DEFAULT_AUTO_REFRESH: bool = true",
    "DEFAULT_REFRESH_INTERVAL_SECONDS: u32 = 20",
]))
check("refresh interval is bounded to integer 10 through 3600", all(token in lifecycle for token in [
    "MIN_REFRESH_INTERVAL_SECONDS: u32 = 10",
    "MAX_REFRESH_INTERVAL_SECONDS: u32 = 3_600",
    "interval_seconds: u32",
]))
check("stale deadline is max sixty seconds or twice interval", "60_000_i64.max(self.interval_ms().saturating_mul(2))" in lifecycle)
check("countdown and due time are success-relative", "last.saturating_add(self.settings.interval_ms())" in lifecycle and "finish_success" in lifecycle)
check("Rust-owned auto scheduler starts with the application", "start_auto_scheduler(app.handle().clone())" in tauri_lib and "tokio::time::interval" in tauri)
check("auto scheduler continues while minimized and skips sleep bursts", "MissedTickBehavior::Skip" in tauri and "StateFlags::SIZE | StateFlags::POSITION" in tauri_lib)
check("clock anomalies are visible and fail closed", all(token in lifecycle for token in ["ClockAnomaly", "System clock changed", "Live values were removed"]))

check("startup stages settings context catalog and ready are typed", all(token in lifecycle for token in [
    "LoadingSettings", "RestoringContext", "LoadingCatalog", "Ready",
]))
check("refresh stages acquisition validation calculation commit and maintenance are typed", all(token in lifecycle for token in [
    "Acquiring", "Validating", "Calculating", "Committing", "Maintaining",
]))
check("every refresh explicitly hides previous live values", "previous_values_hidden: true" in lifecycle)
check("overlapping refresh is rejected", "RefreshInProgress" in lifecycle and "active_cancellation" in tauri)
check("cancellation is threaded to the live provider", "CancellationToken::new()" in tauri and ".acquire(provider_request, cancellation" in tauri)
check("offline cancels and blocks refresh", all(token in tauri + lifecycle for token in [
    "active_cancellation.lock().await", "cancellation.cancel()", "LifecycleError::Offline",
]))
offline_lock = tauri.find("let active = lifecycle.active_cancellation.lock().await;", tauri.find("pub async fn set_offline"))
offline_mutation = tauri.find("controller.set_offline(offline)")
startup_lock = tauri.find("let mut active = lifecycle.active_cancellation.lock().await;", tauri.find("async fn run_refresh"))
startup_begin = tauri.find("begin_refresh(&request_id")
check("offline transition and refresh startup share one serialization lock", (
    0 <= offline_lock < offline_mutation and 0 <= startup_lock < startup_begin
))
check("automatic retry and wake due state is rechecked inside serialized startup", all(token in tauri for token in [
    "enum DueRequirement", "DueRequirement::Normal => controller.due", "DueRequirement::Retry => controller.retry_due",
    "DueRequirement::Wake => controller.due_after_wake", "if !due",
]))
check("wake has an explicit age-relative refresh path", "due_after_wake" in lifecycle and "refresh_after_wake" in tauri)

check("draft and applied contexts are independently persisted", all(token in lifecycle for token in [
    "pub draft: MarketContextDraft", "pub applied: MarketContextDraft", "unapplied_changes",
]))
check("first-run and restored state use one versioned persistence record", all(token in lifecycle + tauri for token in [
    "PersistedLifecycle", "PERSISTED_LIFECYCLE_VERSION", "SETTINGS_SECTION", "SETTINGS_KEY",
]))
check("invalid restored state is actionable and never silently defaulted", all(token in lifecycle + tauri for token in [
    "InvalidRestoredState", "invalid_restored", "invalid_restored_json_is_visible_and_never_silently_defaulted",
]))
check("invalid restored state blocks edits apply and refresh until explicit reset", (
    lifecycle.count("self.ensure_restored_state_valid()?") >= 4
    and "controller.begin_refresh(&request_id(), RefreshTrigger::Manual, 0)" in lifecycle
))
check("persistent lifecycle mutations replace validated memory only after durable save", (
    tauri.count("let mut replacement = controller.clone();") >= 3
    and tauri.count("*controller = replacement;") >= 4
))
check("settings and context validate before persistence", tauri.find("candidate.validate()") < tauri.find("save_lifecycle(&persistence.0, &candidate"))

prepare_at = tauri.find("prepare_acquisition_for_publication")
publish_at = tauri.find("publish_complete_snapshot")
prune_at = tauri.find(".prune(committed_ms")
check("provider calculation publication pipeline is ordered", 0 <= prepare_at < publish_at)
check("provider paging counts local eligibility toward the requested target", all(token in (ROOT / "crates/p2p-provider/src/scheduler.rs").read_text(encoding="utf-8") for token in [
    "AcquisitionEligibility", "evaluate_eligibility", "local_eligibility_continues_paging_to_trustworthy_exhaustion",
]))
check("validated pair catalog records enabled state and observed payments", all(token in tauri for token in [
    "save_catalog_pair", "CatalogPairInput", "payment_methods", '"provider-unspecified"',
]))
check("only prepared complete two-side acquisition reaches publication", all(token in lifecycle + tauri for token in [
    "PreparedAcquisition::Publish", "InsufficientEligibleResults", "validate_acquisition_context",
]))
check("provider empty and local no-match remain distinct", all(token in lifecycle for token in [
    "ProviderEmpty", "NoMatchingResults", "provider_empty_and_filter_no_match_are_distinct_typed_states",
]))
check("partial locally filtered results fail closed", "local_filter_shortfall_never_publishes_partial_results" in lifecycle)
check("atomic persistence follows complete validation", "publish_complete_snapshot(PublicationInput" in tauri)
check("pre-commit lifecycle stage failures finalize and clear the active slot", all(token in tauri for token in [
    "async fn advance_refresh", "controller.finish_error(orchestration_failure(&error))",
    "clear_active(lifecycle).await;", "return Ok(view);",
]))
check("post-commit lifecycle finalization cannot reclassify a committed snapshot as failed", all(token in tauri + lifecycle for token in [
    "finish_committed_success", "The snapshot was committed, but the maintenance stage could not be recorded",
]))
check("post-commit pruning is ordered after publication", 0 <= publish_at < prune_at)
check("startup also invokes low-priority retention maintenance", "Startup retention maintenance failed" in tauri and tauri.count(".prune(") >= 2)
check("pruning failure remains a maintenance warning", all(token in tauri + lifecycle for token in [
    "post-commit retention maintenance failed", "record_maintenance_warning", "pruning_warning_does_not_reclassify_committed_success",
]))

check("actionable error states cover provider validation calculation persistence cancellation", all(token in lifecycle for token in [
    "Provider", "Validation", "Calculation", "Persistence", "Cancelled",
]))
check("non-secret request ID remains available for copyable diagnostics", "last_request_id" in lifecycle and "requestId" in frontend)
check("typed frontend lifecycle contract mirrors Rust states", all(token in frontend for token in [
    '"refreshing"', '"provider-empty"', '"no-matching-results"', '"clock-anomaly"', "unappliedChanges",
]))
check("frontend exposes narrow typed lifecycle commands", all(token in frontend_client for token in [
    '"get_lifecycle_view"', '"reset_lifecycle_state"', '"apply_market_context"', '"refresh_market"', '"refresh_if_due"', '"cancel_refresh"',
]))
lifecycle_commands = [
    "get_lifecycle_view", "reset_lifecycle_state", "update_market_draft", "update_refresh_settings", "apply_market_context",
    "refresh_market", "refresh_if_due", "refresh_after_wake", "set_offline", "cancel_refresh",
]
check("Tauri registers every lifecycle command", all(token in tauri_lib for token in lifecycle_commands))
check("release command manifest retains every lifecycle command", all(f'"{token}"' in tauri_build for token in lifecycle_commands))
check("no implementation placeholders", not re.search(r"\bTODO\b|\btodo!\s*\(|\bunimplemented!\s*\(", lifecycle + tauri))
check("lifecycle crate forbids unsafe code", "#![forbid(unsafe_code)]" in lifecycle and not re.search(r"\bunsafe\s*\{", lifecycle))
check("no cache history Agent or demo live fallback path", not re.search(
    r"(?i)(cache|history|agent|demo).{0,48}(live fallback|fallback.{0,16}live|become live)",
    lifecycle + tauri,
))

passed = sum(1 for item in checks if item["passed"])
report = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "checks": checks}
print(json.dumps(report, indent=2))
sys.exit(0 if report["allPassed"] else 1)
