use crate::contracts::{HostPlatform, PrerequisiteStatus, RuntimePrerequisite};

pub fn host_platform() -> HostPlatform {
    #[cfg(target_os = "windows")]
    return HostPlatform::Windows;
    #[cfg(target_os = "linux")]
    return HostPlatform::Linux;
    #[cfg(target_os = "macos")]
    return HostPlatform::Macos;
    #[allow(unreachable_code)]
    HostPlatform::Unknown
}

#[cfg(not(target_os = "windows"))]
pub fn detect_runtime_prerequisite() -> RuntimePrerequisite {
    RuntimePrerequisite {
        name: "Platform system webview".to_owned(),
        mode: "system".to_owned(),
        status: PrerequisiteStatus::NotApplicable,
        version: None,
        remediation: None,
    }
}

#[cfg(target_os = "windows")]
pub fn detect_runtime_prerequisite() -> RuntimePrerequisite {
    use webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::core::{PCWSTR, PWSTR};

    let mut raw_version = PWSTR::null();
    let result =
        unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut raw_version) };

    if result.is_ok() && !raw_version.is_null() {
        let version = unsafe { raw_version.to_string().ok() };
        unsafe { CoTaskMemFree(Some(raw_version.as_ptr().cast())) };
        RuntimePrerequisite {
            name: "Microsoft Edge WebView2".to_owned(),
            mode: "system".to_owned(),
            status: PrerequisiteStatus::Available,
            version,
            remediation: None,
        }
    } else {
        RuntimePrerequisite {
            name: "Microsoft Edge WebView2".to_owned(),
            mode: "system".to_owned(),
            status: PrerequisiteStatus::Missing,
            version: None,
            remediation: Some(
                "Install Microsoft Edge WebView2 Runtime from Microsoft, then restart P2P Desk."
                    .to_owned(),
            ),
        }
    }
}

#[cfg(target_os = "windows")]
pub fn show_native_startup_error(message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;

    let title = HSTRING::from("P2P Desk — startup blocked");
    let body = HSTRING::from(message);
    unsafe {
        MessageBoxW(None, &body, &title, MB_OK | MB_ICONERROR);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn show_native_startup_error(_message: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_compiled_host_platform() {
        #[cfg(target_os = "linux")]
        assert_eq!(host_platform(), HostPlatform::Linux);
        #[cfg(target_os = "windows")]
        assert_eq!(host_platform(), HostPlatform::Windows);
        #[cfg(target_os = "macos")]
        assert_eq!(host_platform(), HostPlatform::Macos);
    }
}
