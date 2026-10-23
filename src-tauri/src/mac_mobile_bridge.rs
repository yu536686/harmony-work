//! AppKit phone-management sheet. Credentials stay in the shared bridge controller.
#[cfg(target_os = "macos")]
use std::ffi::{c_char, c_void, CString};
#[cfg(target_os = "macos")]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileBridgePhone {
    desktop_device_id: String,
    mobile_device_id: String,
    mobile_name: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileBridgePanelState {
    pairing_uri: String,
    status: String,
    error: String,
    busy: bool,
    linked: bool,
    phones: Vec<MobileBridgePhone>,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn hw_show_mobile_bridge(
        window: *mut c_void,
        webview: *mut c_void,
        options: *const c_char,
    ) -> bool;
    fn hw_update_mobile_bridge(window: *mut c_void, options: *const c_char) -> bool;
    fn hw_close_mobile_bridge(window: *mut c_void, panel_id: *const c_char);
}

fn set_panel(
    window: tauri::WebviewWindow,
    panel_id: String,
    state: MobileBridgePanelState,
    show: bool,
) -> Result<(), String> {
    if window.label() != "main" || panel_id.len() > 128 {
        return Err("手机管理仅限主窗口".into());
    }
    #[cfg(target_os = "macos")]
    {
        let payload = serde_json::json!({ "panelId": panel_id, "state": state }).to_string();
        if payload.len() > 128 * 1024 {
            return Err("手机管理信息过长".into());
        }
        let options = CString::new(payload).map_err(|error| error.to_string())?;
        let succeeded = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&succeeded);
        window
            .with_webview(move |webview| unsafe {
                let result = if show {
                    hw_show_mobile_bridge(webview.ns_window(), webview.inner(), options.as_ptr())
                } else {
                    hw_update_mobile_bridge(webview.ns_window(), options.as_ptr())
                };
                outcome.store(result, Ordering::Relaxed);
            })
            .map_err(|error| error.to_string())?;
        if !succeeded.load(Ordering::Relaxed) {
            return Err("Native mobile bridge panel is unavailable".into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, panel_id, state, show);
        Err("Native mobile bridge panel is only available on macOS".into())
    }
}

#[tauri::command]
pub fn show_macos_mobile_bridge(
    window: tauri::WebviewWindow,
    panel_id: String,
    state: MobileBridgePanelState,
) -> Result<(), String> {
    set_panel(window, panel_id, state, true)
}

#[tauri::command]
pub fn update_macos_mobile_bridge(
    window: tauri::WebviewWindow,
    panel_id: String,
    state: MobileBridgePanelState,
) -> Result<(), String> {
    set_panel(window, panel_id, state, false)
}

#[tauri::command]
pub fn close_macos_mobile_bridge(
    window: tauri::WebviewWindow,
    panel_id: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let panel_id = CString::new(panel_id).map_err(|error| error.to_string())?;
        window
            .with_webview(move |webview| unsafe {
                hw_close_mobile_bridge(webview.ns_window(), panel_id.as_ptr());
            })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, panel_id);
    Ok(())
}
