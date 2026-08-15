# Foundation Security Policy

## Renderer

- Production CSP permits only bundled self assets plus Tauri IPC.
- No `unsafe-eval`, production inline styles/scripts, remote code, frames, forms, external fonts, or arbitrary connections.
- Tauri global API injection is disabled; prototype freezing is enabled.
- Provider-controlled text will be rendered as text only; unsafe DOM sinks are prohibited.

## Native capabilities

- One local window: `main`.
- One allowed application command: `get_bootstrap_info`.
- Tauri removes unused commands during release build.
- No shell, HTTP, filesystem, updater, dialog, notification, clipboard, global-shortcut, process, service, or sidecar plugin.
- Automatic window state is Rust-owned; its frontend commands are removed and not granted.

## Network

Provider access exists only in the trusted Rust `p2p-provider` crate. It has three fixed HTTPS destination constants, rejects redirects, bounds time/body/retry/rate behavior, validates exact contracts, and fails closed. The webview still has no network API and production `connect-src` remains limited to Tauri IPC. See [Experimental provider contract](provider.md).

## Storage

Gate 1 creates only the local product/state directory. Database/log/report/backup behavior is implemented and tested in later gates. The executable directory is never an operational storage root.

## Build and publication

- GitHub Actions has read-only repository contents permission during verification/build jobs.
- Third-party actions are pinned to immutable commit hashes.
- Normal workflow jobs use the short-lived repository-scoped built-in `GITHUB_TOKEN`; no personal access token is stored in source or required by the application.
- Generated binaries are workflow/release artifacts, not source-tree commits.
- Public-source preparation excludes local gate evidence, working records, credentials, generated output, and private references.

## Audit

`scripts/security_audit.py` deterministically checks the current CSP, capability, manifest, code-sink, network, shell, window and data-path invariants. Dependency vulnerability and license scans are recorded in local Gate evidence. Public repository sanitization and workflow permission/action-pin checks are required before each push/release.
