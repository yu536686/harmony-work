#[tauri::command]
pub fn show_macos_update(
    window: tauri::WebviewWindow,
    snapshot: serde_json::Value,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("更新操作仅限主窗口".into());
    }
    #[cfg(target_os = "macos")]
    {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        extern "C" {
            fn hw_show_update(
                window: *mut std::ffi::c_void,
                webview: *mut std::ffi::c_void,
                json: *const std::ffi::c_char,
            ) -> bool;
        }
        let payload = snapshot.to_string();
        if payload.len() > 128 * 1024 {
            return Err("更新信息过长".into());
        }
        let json = std::ffi::CString::new(payload).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let result = shown.clone();
        window
            .with_webview(move |webview| unsafe {
                result.store(
                    hw_show_update(webview.ns_window(), webview.inner(), json.as_ptr()),
                    Ordering::Relaxed,
                );
            })
            .map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) {
            return Err("无法打开更新窗口".into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = snapshot;
        Err("原生更新窗口仅支持 macOS".into())
    }
}

#[tauri::command]
pub fn close_macos_update(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn hw_close_update(window: *mut std::ffi::c_void);
        }
        window
            .with_webview(move |webview| unsafe {
                hw_close_update(webview.ns_window());
            })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
    Ok(())
}
