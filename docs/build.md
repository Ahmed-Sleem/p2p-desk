# Reproducible Build and Verification

## Required versions

- Node.js 24.18.1 with npm 11.16.0
- Rust 1.97.1 via rustup, with rustfmt and clippy
- On Windows: x86_64 MSVC C++ Build Tools and system Microsoft Edge WebView2
- On Intel macOS: macOS 12 or later, Xcode command-line tools, and Rust target `x86_64-apple-darwin`
- On Debian development hosts: the Tauri 2 packages listed in the official prerequisites

Do not use Node 20; it is EOL. Do not replace exact versions or lockfiles silently.

## Locked install and frontend verification

```bash
node --version
npm --version
npm ci --ignore-scripts
npm exec tauri -- --version
npm run format:check
npm run lint
npm run typecheck
npm run test:run
npm run build:web
npm audit --audit-level=moderate
```

## Rust verification

```bash
rustc --version
cargo --version
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets --all-features
cargo build --manifest-path src-tauri/Cargo.toml --locked
cargo audit --file src-tauri/Cargo.lock
```

## Desktop builds

Windows or Linux direct binary:

```bash
npm run tauri:build
```

Native Intel macOS application bundle:

```bash
rustup target add x86_64-apple-darwin
npm exec tauri -- build --target x86_64-apple-darwin --bundles app
```

`tauri build --no-bundle` produces the direct Windows/Linux binary and no installer. The macOS platform configuration produces `P2P Desk.app` with minimum system version 12.0. Linux output is development evidence only. GitHub Actions runs Linux verification, Windows x64 build/checks, and a native `macos-15-intel` build/check in parallel. CI artifacts do not replace final Windows 10/11 and supported Intel macOS runtime smoke testing.

## Sandbox helper

`scripts/use-sandbox-toolchains.sh` sets paths for the excluded local cache used by the Arena Linux workspace. It is not required on a correctly provisioned developer machine.
