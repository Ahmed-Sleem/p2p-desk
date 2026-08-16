# Foundation Architecture

## Boundary

`app/` contains the React/TypeScript presentation shell. `src-tauri/` contains the trusted Rust process. The frontend may call only commands registered in `build.rs`, the Tauri invoke handler, and `capabilities/main.json`.

Current command surface:

- `get_bootstrap_info` — returns non-sensitive product/build/window/runtime/data-root metadata.

The frontend has no filesystem, HTTP, shell, dialog, updater, notification, account, credential, or provider capability.

## Central sources

- Rust product/build/window constants: `src-tauri/src/contracts.rs`.
- Experimental source policy, contracts, scheduling, circuits, Agent isolation and pair checks: `crates/p2p-provider/` and `docs/provider.md`.
- SQLite schema, atomic publication, pseudonymization, retention, cost versions, migrations, backup/restore, and clear scopes: `crates/p2p-persistence/`, `docs/persistence.md`, and `docs/persistence-schema.md`.
- TypeScript IPC shape: `app/src/ipc/contracts.ts`.
- IPC invocation/defensive error normalization: `app/src/ipc/client.ts`.
- Window/security/base Windows bundle policy: `src-tauri/tauri.conf.json`.
- Intel macOS application-bundle policy: `src-tauri/tauri.macos.conf.json`.
- Capability boundary: `src-tauri/capabilities/main.json`.
- Tool versions: `.node-version`, `rust-toolchain.toml`, package manifests and lockfiles.

Later gates must extend these sources rather than duplicate them.

## Provider runtime

The Tauri process constructs and owns one `LiveProviderRuntime`. Its primary adapter uses fixed HTTPS destinations, exact response normalization, one serialized acquisition graph, one globally paced request gate, bounded retries, cancellation, and global timed/persistent circuits. Optional Agent metadata and quote types are structurally separate and cannot become primary ads. Gate 3 does not widen the current frontend command surface; lifecycle/state commands and event wiring are added with their UI orchestration gate.

## Persistence runtime

During Tauri setup, Rust resolves the product data root and opens one shared `PersistenceStore`. The store owns the single application SQLite connection behind a serialized trusted boundary. It enables foreign keys, WAL, full synchronous durability, bounded busy handling, migration/integrity checks, the managed page cap, and the separate pseudonym key before the application becomes ready. No persistence command is exposed to the frontend yet; typed lifecycle commands are added with the state-orchestration gate.

Only a complete validated provider `Acquisition` can enter atomic publication. Raw source identifiers are transformed locally, public nicknames and provider bodies are omitted, and exact values remain decimal text. See the persistence documents for the schema, retention, and rollback-capable backup contract.

## Window state

The first window is 1280×800, minimum 1024×700, native-decorated, normal and work-area constrained. The official window-state plugin stores only normal size and position. It is attached from Rust and writes to the product’s local-data `state/window-state.json`; no frontend plugin permission is granted. Maximize/fullscreen/visibility/decorations are deliberately not restored.

## Operational data path

Rust resolves the per-user OS local-data directory and appends `P2P Desk`. On Windows this produces `%LOCALAPPDATA%\P2P Desk`; on macOS it uses the corresponding user Application Support location resolved by Tauri. No state is stored beside the executable or application bundle.

## Runtime prerequisite

Windows preflight calls the WebView2 loader API before Tauri starts. A missing runtime produces a native blocking error with Microsoft WebView2 remediation and exits without a webview. The bundler install mode is `skip`; P2P Desk never downloads or embeds a fixed runtime. Intel macOS builds use the operating system webview and declare macOS 12.0 as the minimum supported version.

## Build identity

The bootstrap contract exposes app version, debug/release profile, schema version, calculation version and provider-adapter version. All begin at 1 and are centralized for later migrations/replay/report metadata.
