#!/usr/bin/env python3
"""Deterministic Gate 6 design, accessibility, and no-fallback invariants."""
from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "app/src"
APPROVED = ROOT / "design/p2p-desk-production-ui-approval-v2.html"
app = (APP / "App.tsx").read_text(encoding="utf-8")
styles = (APP / "styles.css").read_text(encoding="utf-8")
tokens = (APP / "ui/tokens.css").read_text(encoding="utf-8")
shell = (APP / "ui/AppShell.tsx").read_text(encoding="utf-8")
context = (APP / "ui/ContextBar.tsx").read_text(encoding="utf-8")
pages = (APP / "ui/PageContent.tsx").read_text(encoding="utf-8")
state = (APP / "ui/StateView.tsx").read_text(encoding="utf-8")
help_ui = (APP / "ui/MetricHelp.tsx").read_text(encoding="utf-8")
filters = (APP / "ui/AdvancedFilters.tsx").read_text(encoding="utf-8")
content = (APP / "ui/content.ts").read_text(encoding="utf-8")
all_ui = "\n".join([app, styles, tokens, shell, context, pages, state, help_ui, filters, content])
checks: list[dict[str, object]] = []

def check(name: str, condition: bool, detail: str = "") -> None:
    checks.append({"name": name, "passed": bool(condition), "detail": detail})

approved_hash = hashlib.sha256(APPROVED.read_bytes()).hexdigest()
check("approved production-v2 authority remains immutable", approved_hash == "34a11bbffc8f51a0ea3842f62e124efe0ba03220d4c222904bc8b185a86a9d13", approved_hash)
check("central black white lime token source", all(token in tokens for token in ["--color-canvas: #f7f8fa", "--color-surface: #ffffff", "--color-surface-dark: #0d0d0d", "--color-accent: #6dfd5f", "--rail-width: 54px", "--sidebar-width: 190px"]))
check("obsolete evergreen gold palette removed from production CSS", not re.search(r"#123c34|#ba8a2f|evergreen|gold", styles + tokens, re.I))
check("density uses tokens and never CSS zoom", "--control-height: 37px" in tokens and not re.search(r"(^|[;{])\s*zoom\s*:", styles + tokens, re.M))
check("small shade-only interaction feedback", all(token in styles for token in ["background: var(--color-white-hover)", "background: var(--color-control-hover)", "background: var(--color-accent-hover)"]))
check("six centralized navigation pages", all(f'"{page}"' in content for page in ["overview", "offers", "analysis", "history", "health", "settings"]) and "aria-current" in shell)
check("full viewport shell and bounded internal scroll", all(token in styles for token in ["width: 100vw", "height: 100vh", ".workspace-scroll", "overflow: auto"]))
check("scrolling remains functional while chrome is hidden", all(token in styles for token in ["::-webkit-scrollbar", "scrollbar-width: none", "-ms-overflow-style: none"]))
check("responsive rail drawer and 200 percent reflow foundation", all(token in styles for token in ["@media (max-width: 1180px)", "@media (max-width: 900px)", "@media (max-width: 600px)", "position: fixed"]))
check("reduced motion and forced colors", "prefers-reduced-motion: reduce" in styles and "forced-colors: active" in styles)
check("skip link landmarks and visible keyboard focus", "skip-link" in shell and 'href="#workspace"' in shell and "focus-visible" in styles and "<main" in shell)
check("shared context consumes typed lifecycle draft", "MarketContextDraft" in context and all(token in context for token in ["Transaction amount", "Payment context", "Merchant filters", "Unapplied changes", "Apply"]))
check("advanced controls cover approved context dimensions", all(token in filters for token in ["Market pair", "Payment context", "Merchant thresholds", "Results per side", "Maximum Buy price", "Minimum Sell price", "Pro merchants only"]))
check("settings use trusted persisted refresh contract", "updateSettings" in pages and "Auto-refresh" in pages and "Refresh interval" in pages)
check("source risk is localized to health and settings", "Experimental source" in pages and "Source contract" in pages and "Experimental source" not in shell and "Experimental source" not in context)
check("equal Buy and Sell foundations share one component treatment", pages.count('title="Buy asset"') == 1 and pages.count('title="Sell asset"') == 1 and "metric-foundations" in pages)
check("typed startup refresh empty offline and error boundaries", all(token in pages + state for token in [
    'status.kind === "loading"', 'status.kind === "refreshing"', 'status.kind === "empty"',
    'status.kind === "error"', 'status.failure.kind === "offline"', "Previous live values are hidden",
]))
check("actionable error actions and safe diagnostics", all(token in pages + app for token in ["Retry", "Edit context", "Data Health", "Copy diagnostics", "requestId", "maintenanceWarning"]))
check("metric help includes meaning calculation and exclusions", all(token in help_ui for token in ["What it means", "How calculated", "Excluded", "Read-only"]))
check("native accessible dialog semantics", "<dialog" in help_ui and "aria-labelledby" in help_ui and "showModal" in help_ui and "onClose" in filters)
check("repeated metric dialogs use unique accessible names", "useId" in help_ui and "aria-labelledby={titleId}" in help_ui and "id={titleId}" in help_ui)
check("no unsafe HTML sink", "dangerouslySetInnerHTML" not in all_ui and ".innerHTML" not in all_ui)
check("no frontend network or remote asset path", not re.search(r"\bfetch\s*\(|XMLHttpRequest|WebSocket|https?://|@import\s+url", all_ui))
check("no runtime fabricated live values", not re.search(r"\b50\.84\b|\b50\.47\b|Merchant 0[1-9]|demoData|sampleData|mockData", all_ui))
check("no execution or provider-handoff control", not re.search(r"place order|execute order|open on binance|provider handoff", all_ui, re.I))
check("no global experimental complete auto-refresh cluster", "Experimental/Complete/Auto-refresh" not in all_ui and "source-badge" not in all_ui)
check("central reusable component library exists", all((APP / f"ui/{name}").is_file() for name in ["tokens.css", "primitives.tsx", "Icon.tsx", "AppShell.tsx", "StateView.tsx", "MetricHelp.tsx", "ContextBar.tsx"]))
check("no implementation placeholders", not re.search(r"\bTODO\b|\btodo!\s*\(|\bunimplemented!\s*\(|lorem ipsum", all_ui, re.I))

passed = sum(1 for item in checks if item["passed"])
report = {"passed": passed, "total": len(checks), "allPassed": passed == len(checks), "approvedAuthoritySha256": approved_hash, "checks": checks}
print(json.dumps(report, indent=2))
sys.exit(0 if report["allPassed"] else 1)
