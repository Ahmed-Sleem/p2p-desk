mod commands;
pub mod contracts;
pub use p2p_domain as domain;
pub use p2p_lifecycle as lifecycle;
pub use p2p_persistence as persistence;
pub use p2p_provider as provider;
mod lifecycle_commands;
mod platform;

use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use contracts::{PrerequisiteStatus, RuntimePrerequisite};
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

#[derive(Clone)]
pub struct RuntimeState(pub RuntimePrerequisite);

#[derive(Clone)]
pub struct ProviderRuntimeState(pub provider::LiveProviderRuntime);

#[derive(Clone)]
pub struct PersistenceRuntimeState(pub Arc<persistence::PersistenceStore>);

#[derive(Clone)]
pub struct LifecycleRuntimeState {
    controller: Arc<tokio::sync::Mutex<lifecycle::LifecycleController>>,
    active_cancellation: Arc<tokio::sync::Mutex<Option<tokio_util::sync::CancellationToken>>>,
}

#[derive(Debug)]
pub enum StartupError {
    MissingPrerequisite(String),
    Provider(String),
    Tauri(tauri::Error),
    Io(std::io::Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrerequisite(message) => formatter.write_str(message),
            Self::Provider(message) => {
                write!(formatter, "Provider initialization failed: {message}")
            }
            Self::Tauri(error) => write!(formatter, "Tauri startup failed: {error}"),
            Self::Io(error) => write!(formatter, "Local application state is unavailable: {error}"),
        }
    }
}

impl std::error::Error for StartupError {}

impl From<tauri::Error> for StartupError {
    fn from(value: tauri::Error) -> Self {
        Self::Tauri(value)
    }
}

impl From<std::io::Error> for StartupError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn run() -> Result<(), StartupError> {
    let runtime = platform::detect_runtime_prerequisite();
    if runtime.status == PrerequisiteStatus::Missing {
        return Err(StartupError::MissingPrerequisite(
            runtime.remediation.clone().unwrap_or_else(|| {
                "The required system webview runtime is unavailable.".to_owned()
            }),
        ));
    }

    let provider = provider::LiveProviderRuntime::new()
        .map_err(|error| StartupError::Provider(error.to_string()))?;

    tauri::Builder::default()
        .manage(RuntimeState(runtime))
        .manage(ProviderRuntimeState(provider))
        .setup(|app| {
            let local_data = app.path().local_data_dir()?;
            let data_root = commands::data_root_for(&local_data);
            let state_dir = data_root.join("state");
            std::fs::create_dir_all(&state_dir)?;
            let opened_at_ms = i64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(std::io::Error::other)?
                    .as_millis(),
            )
            .map_err(|_| std::io::Error::other("system time exceeds supported range"))?;
            let versions = persistence::RuntimeVersions::current(env!("CARGO_PKG_VERSION"))?;
            let persistence = Arc::new(persistence::PersistenceStore::open(
                &data_root,
                versions,
                opened_at_ms,
            )?);
            let lifecycle = lifecycle_commands::initialize_lifecycle(&persistence, opened_at_ms)?;
            app.manage(PersistenceRuntimeState(persistence));
            app.manage(lifecycle);
            let state_file = state_dir.join("window-state.json");
            app.handle().plugin(
                tauri_plugin_window_state::Builder::new()
                    .with_filename(state_file.to_string_lossy())
                    .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                    .build(),
            )?;
            lifecycle_commands::start_auto_scheduler(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_bootstrap_info,
            lifecycle_commands::get_lifecycle_view,
            lifecycle_commands::reset_lifecycle_state,
            lifecycle_commands::update_market_draft,
            lifecycle_commands::update_refresh_settings,
            lifecycle_commands::apply_market_context,
            lifecycle_commands::refresh_market,
            lifecycle_commands::refresh_if_due,
            lifecycle_commands::refresh_after_wake,
            lifecycle_commands::set_offline,
            lifecycle_commands::cancel_refresh,
        ])
        .run(tauri::generate_context!())
        .map_err(StartupError::Tauri)
}

pub fn report_startup_error(error: &StartupError) {
    platform::show_native_startup_error(&error.to_string());
}
