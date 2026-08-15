const COMMANDS: &[&str] = &["get_bootstrap_info"];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to generate the P2P Desk Tauri build context");
}
