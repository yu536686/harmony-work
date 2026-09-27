#[cfg(target_os = "macos")]
use std::ffi::c_void;

#[cfg(target_os = "macos")]
extern "C" {
    fn hw_install_native_shell(window: *mut c_void, webview: *mut c_void);
    fn hw_set_shell_state(window: *mut c_void, width: f64, dark: bool);
}

#[cfg(target_os = "macos")]
pub fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    window.with_webview(|webview| unsafe {
        hw_install_native_shell(webview.ns_window(), webview.inner());
    })
}

#[tauri::command]
pub fn set_macos_shell_state(
    window: tauri::WebviewWindow,
    width: f64,
    dark: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        window
            .with_webview(move |webview| unsafe {
                hw_set_shell_state(webview.ns_window(), width, dark);
            })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, width, dark);
    }
    Ok(())
}
