use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

use crate::RuntimeState;
use crate::contracts::{
    AppErrorEnvelope, BootstrapInfo, PRODUCT_NAME, PRODUCT_SUBTITLE, WINDOW_POLICY,
    current_build_info,
};
use crate::platform::host_platform;

pub fn data_root_for(local_data_dir: &Path) -> PathBuf {
    local_data_dir.join(PRODUCT_NAME)
}

#[tauri::command]
pub fn get_bootstrap_info(
    app: AppHandle,
    runtime: tauri::State<'_, RuntimeState>,
) -> Result<BootstrapInfo, AppErrorEnvelope> {
    let local_data_dir = app.path().local_data_dir().map_err(|error| {
        AppErrorEnvelope::storage(format!(
            "The operating-system local data path is unavailable: {error}"
        ))
    })?;
    let data_root = data_root_for(&local_data_dir);

    Ok(BootstrapInfo {
        product_name: PRODUCT_NAME,
        subtitle: PRODUCT_SUBTITLE,
        host_platform: host_platform(),
        data_root: data_root.to_string_lossy().into_owned(),
        build: current_build_info(),
        window_policy: WINDOW_POLICY,
        runtime: runtime.0.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_root_is_product_named_and_not_executable_relative() {
        let base = Path::new("/local-data");
        assert_eq!(data_root_for(base), PathBuf::from("/local-data/P2P Desk"));
    }
}
