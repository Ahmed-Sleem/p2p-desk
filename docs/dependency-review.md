# Dependency and Advisory Review

Review date: 2026-08-14 (Africa/Cairo)

## Inventory and licenses

The locked inventory contains 676 unique ecosystem/name/version components: 437 Cargo and 239 npm. Registry/package metadata reports a license for every component. The initial automated scan found no AGPL, GPL-3.0, or GPLv3 candidate expression. This is an initial metadata screen, not a replacement for the final release notices and license-text package.

Evidence:

- `evidence/gate_01_dependency_inventory.csv`
- `evidence/gate_01_dependency_summary.json`

## Vulnerabilities

- npm audit: 0 total across info, low, moderate, high and critical.
- RustSec cargo-audit: no vulnerability advisory matched the lockfile.

## RustSec informational warnings

Cargo-audit also reported 16 unmaintained-package warnings and one unsoundness warning:

1. Ten GTK 3 binding advisories plus `proc-macro-error` are transitive through Tauri’s Linux WebKit/GTK development stack. They are absent from the required Windows target graph.
2. `RUSTSEC-2024-0429` concerns `glib::VariantStrIter`. `glib` is absent from the Windows target graph; P2P Desk has no direct `glib` dependency and does not use that iterator. The Linux binary is development evidence, not a release deliverable.
3. Five `unic-*` packages are unmaintained and arrive through Tauri’s `urlpattern` dependency on all targets. RustSec reports maintenance status, not a vulnerability, and the current pinned Tauri release offers no project-level replacement.

Disposition for Gate 1: reviewed, scoped, and retained as non-vulnerability maintenance signals. Re-scan at every dependency review and before release; any vulnerability or newly applicable soundness path blocks the next release gate.

Evidence:

- `evidence/gate_01_cargo_audit.json`
- `evidence/gate_01_cargo_advisory_paths.txt`
- `evidence/gate_01_windows_advisory_scope.txt`

## Gate 3 provider update — 2026-08-15

The trusted provider crate adds exact-pinned Reqwest 0.13.4, Tokio 1.53.1, Tokio Util 0.7.19, and their locked TLS/HTTP dependencies. Reqwest uses Rustls with the platform certificate verifier, HTTPS-only requests, and no redirect, cookie, compression, multipart, proxy-discovery, or frontend HTTP plugin feature.

The provider-only locked graph contains 168 dependency name/version components across all target metadata. Registry metadata reports a license for every component; no AGPL/GPL-3/GPLv3 candidate expression was found. Cargo-audit reports zero vulnerabilities and zero informational warnings for the provider lockfile. The integrated Tauri lockfile still reports zero vulnerabilities plus the same previously reviewed 16 unmaintained and one Linux GTK/glib unsound informational warnings; the provider addition introduced no matched advisory.

Local evidence is kept outside the public repository:

- `evidence/gate_03_provider_dependency_inventory.json`
- `evidence/gate_03_provider_cargo_audit.json`
- `evidence/gate_03_integrated_cargo_audit.json`
