mod commands;
pub mod contracts;
pub use p2p_domain as domain;
mod platform;

use std::fmt;

use contracts::{PrerequisiteStatus, RuntimePrerequisite};
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

#[derive(Clone)]
pub struct RuntimeState(pub RuntimePrerequisite);

#[derive(Debug)]
pub enum StartupError {
    MissingPrerequisite(String),
    Tauri(tauri::Error),
    Io(std::io::Error),
}

impl fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPrerequisite(message) => formatter.write_str(message),
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

    tauri::Builder::default()
        .manage(RuntimeState(runtime))
        .setup(|app| {
            let local_data = app.path().local_data_dir()?;
            let state_dir = commands::data_root_for(&local_data).join("state");
            std::fs::create_dir_all(&state_dir)?;
            let state_file = state_dir.join("window-state.json");
            app.handle().plugin(
                tauri_plugin_window_state::Builder::new()
                    .with_filename(state_file.to_string_lossy())
                    .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                    .build(),
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::get_bootstrap_info])
        .run(tauri::generate_context!())
        .map_err(StartupError::Tauri)
}

pub fn report_startup_error(error: &StartupError) {
    platform::show_native_startup_error(&error.to_string());
}
