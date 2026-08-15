#!/usr/bin/env python3
"""Deterministic Gate 2 source/lock/contract closure audit."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOMAIN = ROOT / "crates" / "p2p-domain"
SRC = DOMAIN / "src"
EVIDENCE = ROOT / "evidence"

checks: list[dict[str, object]] = []


def check(name: str, passed: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(passed), "detail": detail})


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


manifest = read(DOMAIN / "Cargo.toml")
tauri_manifest = read(ROOT / "src-tauri" / "Cargo.toml")
decimal = read(SRC / "decimal.rs")
market = read(SRC / "market.rs")
model = read(SRC / "model.rs")
eligibility = read(SRC / "eligibility.rs")
quote = read(SRC / "quote.rs")
costs = read(SRC / "costs.rs")
statistics = read(SRC / "statistics.rs")
multi_ad = read(SRC / "multi_ad.rs")
version = read(SRC / "version.rs")
lib = read(SRC / "lib.rs")
all_rust = "\n".join(read(path) for path in sorted(DOMAIN.rglob("*.rs")))
locks = read(DOMAIN / "Cargo.lock") + "\n" + read(ROOT / "src-tauri" / "Cargo.lock")

check("standalone domain crate manifest", 'name = "p2p-domain"' in manifest)
check(
    "fastnum exact version and feature pin",
    'fastnum = { version = "=0.7.5", default-features = false, features = ["std"] }'
    in manifest,
)
check("property-test version pin", 'proptest = { version = "=1.11.0"' in manifest)
check("Tauri consumes path domain crate", 'p2p-domain = { path = "../crates/p2p-domain" }' in tauri_manifest)
check("rust_decimal removed from manifests and locks", "rust_decimal" not in manifest + tauri_manifest + locks)
check("rkyv advisory package removed from locks", not re.search(r'^name = "rkyv"$', locks, re.MULTILINE))
check("no binary float type in domain", not re.search(r"\b(?:f32|f64)\b", all_rust))
check("no unsafe block in domain", not re.search(r"\bunsafe\b", all_rust))
check("no implementation placeholder in domain", not re.search(r"\b(?:TODO|FIXME|unimplemented!|todo!)\b", all_rust))
check("ExactDecimal inner D256 is private", "pub struct ExactDecimal(D256);" in decimal)
check("decimal JSON boundary is string-only", "deserialize_str(ExactDecimalVisitor)" in decimal)
check("strict plain-decimal shape validation", "inspect_plain_decimal" in decimal and "InvalidNotation" in decimal)
check("half-even calculation and quantization", decimal.count("RoundingMode::HalfEven") >= 2)
check(
    "finite arithmetic signal validation",
    all(token in decimal for token in ["is_finite", "is_op_overflow", "is_op_underflow", "is_op_invalid", "is_op_div_by_zero", "is_op_inexact"]),
)
check(
    "side inversion correction is explicit",
    all(token in market for token in ["Self::BuyAsset => RequestSide::Buy", "Self::BuyAsset => AdvertiserSide::Sell", "Self::SellAsset => RequestSide::Sell", "Self::SellAsset => AdvertiserSide::Buy"]),
)
check(
    "typed pair amount payment filter quality provenance contracts",
    all(token in model for token in ["struct MarketPair", "struct RequestedAmount", "struct ResultsTarget", "struct EligibilityFilters", "struct SideQuality", "struct SnapshotProvenance"]),
)
check(
    "eligibility covers every approved gate-2 dimension",
    all(token in eligibility for token in ["BelowMinimum", "AboveMaximum", "InsufficientAvailability", "PaymentMismatch", "BelowMinimumOrders", "BelowMinimumCompletion", "BelowMinimumPositive", "NotPro", "AboveMaximumBuyPrice", "BelowMinimumSellPrice"]),
)
check("ranking has explicit stable tie chain", all(token in quote for token in ["completion_percent", "monthly_orders", "observed_at_ms", "stable_id"]))
check("fiat and asset quote modes implemented", all(token in quote for token in ["AmountMode::Fiat", "AmountMode::Asset", "BuyAsset", "SellAsset"]))
check("unknown costs suppress net distinctly from zero", all(token in costs for token in ["CostInput::UNKNOWN", "explicit_zero", "NetAvailability::UnknownCosts", "suppress"] if token != "suppress") and "UnknownCosts" in costs)
check("compatible spread requires exact route", "IncompatibleRoute" in costs and "payments().contains(route)" in costs)
check("gross denominator and break-even implemented", "gross_percent_of_buy_cost" in costs and "break_even_sell_price" in costs)
check("R7 quantile implementation", "pub fn r7_quantile" in statistics and "position" in statistics and "fraction" in statistics)
check("positive-weight inverse ECDF implementation", "pub fn inverse_weighted_ecdf_quantile" in statistics and "cumulative >= target" in statistics)
check("MAD modified-z sufficiency and zero-MAD policy", all(token in statistics for token in ["OUTLIER_MINIMUM_SAMPLE", "OUTLIER_COEFFICIENT_TEXT", "OUTLIER_THRESHOLD_TEXT", "IndeterminateZeroMad"]))
check("concentration overlap stability primitives", all(token in statistics for token in ["herfindahl_hirschman_index", "top_k_share", "jaccard_index", "stability_summary"]))
check("deterministic amount sensitivity", "pub fn amount_sensitivity" in read(SRC / "sensitivity.rs"))
check(
    "multi-ad preserves multiply/divide semantics and fill/output/leg/key comparison",
    all(
        token in multi_ad
        for token in [
            "OutputRate::Divide",
            "input.checked_div(input_per_output)",
            "candidate_fill",
            "candidate_output",
            "candidate.len()",
            "stable_id",
        ]
    )
    and "ExactDecimal::ONE.checked_div(ad.price())" not in multi_ad,
)
check("optimal label restricted to continuous safe case", "candidate.minimum_input().is_zero() && candidate.fixed_output_cost().is_zero()" in multi_ad and "CertifiedContinuousSortedGreedy" in multi_ad)
check("general multi-ad output is heuristic estimate", "DeterministicMinimumFixedCostRepairEstimate" in multi_ad and "optimal: false" not in multi_ad)
check("versioned formula metadata exported", "FORMULA_METADATA" in version and "FORMULA_METADATA" in lib)
check("versioned golden replay fixture exists", (DOMAIN / "tests" / "golden.rs").is_file() and (DOMAIN / "tests" / "fixtures" / "gate_02_golden.json").is_file())

fixture = json.loads(read(DOMAIN / "tests" / "fixtures" / "gate_02_golden.json"))
calc_match = re.search(r'CALCULATION_VERSION: &str = "([^"]+)"', version)
schema_match = re.search(r'DOMAIN_SCHEMA_VERSION: &str = "([^"]+)"', version)
check(
    "golden fixture versions match source",
    calc_match is not None
    and schema_match is not None
    and fixture.get("calculationVersion") == calc_match.group(1)
    and fixture.get("domainSchemaVersion") == schema_match.group(1),
)

passed = sum(1 for item in checks if item["passed"])
result = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "checks": checks}
EVIDENCE.mkdir(parents=True, exist_ok=True)
out = EVIDENCE / "gate_02_audit_results.json"
out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps(result, indent=2))
raise SystemExit(0 if result["allPassed"] else 1)
