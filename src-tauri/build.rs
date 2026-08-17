const COMMANDS: &[&str] = &[
    "get_bootstrap_info",
    "get_lifecycle_view",
    "reset_lifecycle_state",
    "update_market_draft",
    "update_refresh_settings",
    "apply_market_context",
    "refresh_market",
    "refresh_if_due",
    "refresh_after_wake",
    "set_offline",
    "cancel_refresh",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to generate the P2P Desk Tauri build context");
}
