#!/usr/bin/env python3
"""Deterministic closure invariants for the Gate 3 provider adapter."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROVIDER = ROOT / "crates/p2p-provider"
source_files = sorted((PROVIDER / "src").glob("*.rs"))
sources = "\n".join(path.read_text(encoding="utf-8") for path in source_files)
policy = (PROVIDER / "src/policy.rs").read_text(encoding="utf-8")
contract = (PROVIDER / "src/contract.rs").read_text(encoding="utf-8")
scheduler = (PROVIDER / "src/scheduler.rs").read_text(encoding="utf-8")
transport = (PROVIDER / "src/transport.rs").read_text(encoding="utf-8")
agent = (PROVIDER / "src/agent.rs").read_text(encoding="utf-8")
runtime = (PROVIDER / "src/runtime.rs").read_text(encoding="utf-8")
domain_market = (ROOT / "crates/p2p-domain/src/market.rs").read_text(encoding="utf-8")
tauri_lib = (ROOT / "src-tauri/src/lib.rs").read_text(encoding="utf-8")
tauri_cargo = (ROOT / "src-tauri/Cargo.toml").read_text(encoding="utf-8")
provider_cargo = (PROVIDER / "Cargo.toml").read_text(encoding="utf-8")
example = (PROVIDER / "examples/provider_diagnostic.rs").read_text(encoding="utf-8")
checks: list[dict[str, object]] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(condition), "detail": detail})


check("provider crate and locked integration exist", (PROVIDER / "Cargo.lock").is_file() and 'p2p-provider = { path = "../crates/p2p-provider" }' in tauri_cargo)
check("network dependencies are exact and Rust-owned", 'reqwest = { version = "=0.13.4"' in provider_cargo and "tauri-plugin-http" not in provider_cargo)
check("persistent experimental label and disclosure are centralized", all(token in policy for token in ["Experimental Binance P2P Web", "Unsupported website search contract", "acknowledgement_required: true"]))
check("no-fallback disclosure is explicit", "Cached, historical, Agent, secondary, or fabricated values are never" in policy)

endpoints = re.findall(r'"https://[^"\s]+"', policy)
check("exact HTTPS allowlist has only three provider endpoints", len(endpoints) == 3 and len(set(endpoints)) == 3, ", ".join(endpoints))
check("HTTP client is HTTPS-only and rejects redirects/referer", all(token in transport for token in [".https_only(true)", "Policy::none()", ".referer(false)"]))
check("connect and total request timeouts are 10s and 30s", "Duration::from_secs(10)" in policy and "Duration::from_secs(30)" in policy)
check("response body is bounded while streaming", "4 * 1024 * 1024" in policy and "response.chunk()" in transport and "BodyTooLarge" in transport)

check("corrected user-intent/request/advertiser side mapping remains exact", all(token in domain_market for token in ["Self::BuyAsset => RequestSide::Buy", "Self::BuyAsset => AdvertiserSide::Sell", "Self::SellAsset => RequestSide::Sell", "Self::SellAsset => AdvertiserSide::Buy"]))
check("wrong-side and cross-pair rows are hard failures", all(token in contract for token in ["ContractError::WrongSide", "ContractError::CrossPair", "expected_advertiser_side"]))
check("provider decimals never use binary float types", not re.search(r"\bf(?:32|64)\b|parse::<f(?:32|64)>", sources))
check("numeric JSON tokens parse through ExactDecimal lexemes", "Number(value) => value.to_string()" in contract and "ExactDecimal::from_str" in contract)
check("merchant ratios are bounded and converted exactly", "ratio > ExactDecimal::ONE" in contract and ".checked_mul(ExactDecimal::HUNDRED)" in contract)
check("provider display text strips controls and is bounded", ".filter(|character| !character.is_control())" in contract and ".take(MAX_DISPLAY_TEXT_CHARS)" in contract)
check("unknown fields are isolated and record rejections are categorized", "Vec<Value>" in contract and "RecordRejectionCode" in contract)

check("pagination uses 20-row pages and at most 50 per side", "PAGE_SIZE: u8 = 20" in policy and "MAX_PAGES_PER_SIDE: u8 = 50" in policy)
check("scheduler alternates corrected sides", "[UserIntent::BuyAsset, UserIntent::SellAsset]" in scheduler)
check("one graph and one global request are serialized", "graph_lock" in scheduler and "last_start: Arc<Mutex<Option<Instant>>>" in transport)
check("request starts are spaced by at least 500ms", "Duration::from_millis(500)" in policy and "minimum_gap - elapsed" in transport)
check("deduplication and no-progress detection fail closed", all(token in scheduler for token in ["seen_ids", "RepeatedOrNoProgressPage", "unique_on_page == 0"]))
check("short/empty page requires trustworthy exhaustion", "InconsistentTerminalPage" in scheduler and "provider_exhausted" in scheduler)
check("two-side zero, asymmetric zero, and all-rejected are distinct", all(token in scheduler for token in ["AsymmetricProviderZero", "AllRowsRejected", "provider_total == Some(0)"]))

check("primary retries are capped at three with one/two-second jitter bases", all(token in policy for token in ["MAX_ATTEMPTS: u8 = 3", "FIRST_RETRY_DELAY: Duration = Duration::from_secs(1)", "SECOND_RETRY_DELAY: Duration = Duration::from_secs(2)"]))
check("only approved transient statuses and transport classes retry", "matches!(status, 408 | 500 | 502 | 503 | 504)" in scheduler and "is_retryable_transport" in scheduler)
check("429 and WAF/418 open conservative global circuits", all(token in scheduler for token in ["response.status == 429", "matches!(response.status, 403 | 418)"]) and all(token in policy for token in ["Duration::from_secs(60)", "Duration::from_secs(15 * 60)"]))
check("schema and side failures open persistent circuits", all(token in scheduler for token in ["CircuitReason::SchemaContract", "CircuitReason::SideContract", "open_persistent"]))
check("cancellation covers queue, pacing, request and backoff", sources.count("cancellation.cancelled()") >= 8 and "CancellationToken" in sources)

check("Agent metadata is structurally isolated from primary ads", "cannot be converted" in agent and "-> NormalizedAd" not in agent and "Acquisition {" not in agent and "use crate::contract::NormalizedAd" not in agent)
check("pair check requires a complete primary acquisition", "self.primary" in runtime and "VerifiedPair::from_acquisition" in runtime)
check("Tauri owns one provider runtime without frontend HTTP capability", "ProviderRuntimeState" in tauri_lib and "LiveProviderRuntime::new()" in tauri_lib)
check("live diagnostic emits aggregate fields only", all(token in example for token in ["buy_valid", "sell_valid", "buy_total", "sell_total", "agent_trade_methods"]) and not any(token in example for token in ["public_nickname", "stable_id", "payments()"]))
check("test values are synthetic and no captured fixture is included", "synthetic-ad" in scheduler and "include_bytes!" not in sources and "include_str!" not in sources)
check("public provider policy documentation exists", (ROOT / "docs/provider.md").is_file())

passed = sum(1 for item in checks if item["passed"])
report = {
    "passed": passed,
    "total": len(checks),
    "allPassed": passed == len(checks),
    "checks": checks,
}
print(json.dumps(report, indent=2))
sys.exit(0 if report["allPassed"] else 1)
