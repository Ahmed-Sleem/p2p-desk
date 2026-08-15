# Foundation Architecture

## Boundary

`app/` contains the React/TypeScript presentation shell. `src-tauri/` contains the trusted Rust process. The frontend may call only commands registered in `build.rs`, the Tauri invoke handler, and `capabilities/main.json`.

Current command surface:

- `get_bootstrap_info` — returns non-sensitive product/build/window/runtime/data-root metadata.

The frontend has no filesystem, HTTP, shell, dialog, updater, notification, account, credential, or provider capability.

## Central sources

- Rust product/build/window constants: `src-tauri/src/contracts.rs`.
- TypeScript IPC shape: `app/src/ipc/contracts.ts`.
- IPC invocation/defensive error normalization: `app/src/ipc/client.ts`.
- Window/security/base Windows bundle policy: `src-tauri/tauri.conf.json`.
- Intel macOS application-bundle policy: `src-tauri/tauri.macos.conf.json`.
- Capability boundary: `src-tauri/capabilities/main.json`.
- Tool versions: `.node-version`, `rust-toolchain.toml`, package manifests and lockfiles.

Later gates must extend these sources rather than duplicate them.

## Window state

The first window is 1280×800, minimum 1024×700, native-decorated, normal and work-area constrained. The official window-state plugin stores only normal size and position. It is attached from Rust and writes to the product’s local-data `state/window-state.json`; no frontend plugin permission is granted. Maximize/fullscreen/visibility/decorations are deliberately not restored.

## Operational data path

Rust resolves the per-user OS local-data directory and appends `P2P Desk`. On Windows this produces `%LOCALAPPDATA%\P2P Desk`; on macOS it uses the corresponding user Application Support location resolved by Tauri. No state is stored beside the executable or application bundle.

## Runtime prerequisite

Windows preflight calls the WebView2 loader API before Tauri starts. A missing runtime produces a native blocking error with Microsoft WebView2 remediation and exits without a webview. The bundler install mode is `skip`; P2P Desk never downloads or embeds a fixed runtime. Intel macOS builds use the operating system webview and declare macOS 12.0 as the minimum supported version.

## Build identity

The bootstrap contract exposes app version, debug/release profile, schema version, calculation version and provider-adapter version. All begin at 1 and are centralized for later migrations/replay/report metadata.
