use serde::Serialize;

pub const PRODUCT_NAME: &str = "P2P Desk";
pub const PRODUCT_SUBTITLE: &str = "Read-only P2P decision terminal";
pub const SCHEMA_VERSION: u32 = 1;
pub const CALCULATION_VERSION: u32 = 1;
pub const PROVIDER_ADAPTER_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostPlatform {
    Windows,
    Linux,
    Macos,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrerequisiteStatus {
    Available,
    NotApplicable,
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub app_version: String,
    pub build_profile: BuildProfile,
    pub schema_version: u32,
    pub calculation_version: u32,
    pub provider_adapter_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildProfile {
    Debug,
    Release,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPolicy {
    pub first_width: u32,
    pub first_height: u32,
    pub minimum_width: u32,
    pub minimum_height: u32,
    pub restores_safe_normal_bounds: bool,
    pub native_decorations: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePrerequisite {
    pub name: String,
    pub mode: String,
    pub status: PrerequisiteStatus,
    pub version: Option<String>,
    pub remediation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapInfo {
    pub product_name: &'static str,
    pub subtitle: &'static str,
    pub host_platform: HostPlatform,
    pub data_root: String,
    pub build: BuildInfo,
    pub window_policy: WindowPolicy,
    pub runtime: RuntimePrerequisite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorEnvelope {
    pub code: &'static str,
    pub category: AppErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppErrorCategory {
    Configuration,
    Prerequisite,
    Storage,
    Internal,
}

impl AppErrorEnvelope {
    pub fn storage(message: impl Into<String>) -> Self {
        Self {
            code: "CORE-STORAGE-PATH",
            category: AppErrorCategory::Storage,
            message: message.into(),
            retryable: false,
            request_id: None,
        }
    }
}

pub fn current_build_info() -> BuildInfo {
    BuildInfo {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        build_profile: if cfg!(debug_assertions) {
            BuildProfile::Debug
        } else {
            BuildProfile::Release
        },
        schema_version: SCHEMA_VERSION,
        calculation_version: CALCULATION_VERSION,
        provider_adapter_version: PROVIDER_ADAPTER_VERSION,
    }
}

pub const WINDOW_POLICY: WindowPolicy = WindowPolicy {
    first_width: 1280,
    first_height: 800,
    minimum_width: 1024,
    minimum_height: 700,
    restores_safe_normal_bounds: true,
    native_decorations: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_uses_frontend_contract_casing() {
        let value = serde_json::to_value(AppErrorEnvelope::storage("unavailable"))
            .expect("serialize error envelope");
        assert_eq!(value["code"], "CORE-STORAGE-PATH");
        assert_eq!(value["category"], "storage");
        assert_eq!(value["requestId"], serde_json::Value::Null);
    }

    #[test]
    fn window_policy_matches_the_approved_gate_zero_values() {
        assert_eq!(WINDOW_POLICY.first_width, 1280);
        assert_eq!(WINDOW_POLICY.first_height, 800);
        assert_eq!(WINDOW_POLICY.minimum_width, 1024);
        assert_eq!(WINDOW_POLICY.minimum_height, 700);
        let value = serde_json::to_value(WINDOW_POLICY).expect("serialize window policy");
        assert_eq!(value["restoresSafeNormalBounds"], true);
        assert_eq!(value["nativeDecorations"], true);
    }
}
