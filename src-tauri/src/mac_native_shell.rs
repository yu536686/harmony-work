#[cfg(target_os = "macos")]
use std::ffi::c_void;

#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDropdownItem {
    id: String,
    label: String,
    selected: bool,
    action: bool,
    disabled: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMenuItem {
    id: String,
    label: String,
    disabled: bool,
    selected: bool,
    separator: bool,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn hw_install_native_shell(window: *mut c_void, webview: *mut c_void);
    fn hw_set_shell_state(window: *mut c_void, width: f64, dark: bool);
    fn hw_show_dropdown(window: *mut c_void, webview: *mut c_void, options: *const std::ffi::c_char) -> bool;
    fn hw_close_dropdown(window: *mut c_void, menu_id: *const std::ffi::c_char);
    fn hw_update_dropdown(window: *mut c_void, menu_id: *const std::ffi::c_char, items: *const std::ffi::c_char);
    fn hw_show_menu(window: *mut c_void, webview: *mut c_void, options: *const std::ffi::c_char) -> bool;
    fn hw_show_annotation(window: *mut c_void, webview: *mut c_void, options: *const std::ffi::c_char) -> bool;
    fn hw_close_annotation(window: *mut c_void, annotation_id: *const std::ffi::c_char);
    fn hw_show_info(window: *mut c_void, webview: *mut c_void, options: *const std::ffi::c_char) -> bool;
    fn hw_close_info(window: *mut c_void, info_id: *const std::ffi::c_char);
    fn hw_show_dialog(window: *mut c_void, webview: *mut c_void, options: *const std::ffi::c_char) -> bool;
    fn hw_close_dialog(window: *mut c_void, dialog_id: *const std::ffi::c_char);
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeInfoItem {
    label: String,
    value: String,
}

#[tauri::command]
pub fn show_macos_info(
    window: tauri::WebviewWindow,
    info_id: String,
    title: String,
    empty_label: String,
    items: Vec<NativeInfoItem>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let options = serde_json::json!({
            "infoId": info_id, "title": title, "emptyLabel": empty_label,
            "items": items, "x": x, "y": y, "width": width, "height": height,
        });
        let json = CString::new(options.to_string()).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&shown);
        window.with_webview(move |webview| unsafe {
            outcome.store(hw_show_info(webview.ns_window(), webview.inner(), json.as_ptr()), Ordering::Relaxed);
        }).map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) { return Err("Native info is unavailable".into()); }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, info_id, title, empty_label, items, x, y, width, height);
        return Err("Native info is only available on macOS".into());
    }
    Ok(())
}

#[tauri::command]
pub fn close_macos_info(window: tauri::WebviewWindow, info_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let info_id = CString::new(info_id).map_err(|error| error.to_string())?;
        window.with_webview(move |webview| unsafe {
            hw_close_info(webview.ns_window(), info_id.as_ptr());
        }).map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, info_id);
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn show_macos_dialog(
    window: tauri::WebviewWindow,
    dialog_id: String,
    title: String,
    message: String,
    confirm_label: String,
    cancel_label: String,
    danger: bool,
    alert: bool,
    prompt: bool,
    value: String,
    placeholder: Option<String>,
    extra_label: Option<String>,
    detail: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let options = serde_json::json!({
            "dialogId": dialog_id, "title": title, "message": message,
            "confirmLabel": confirm_label, "cancelLabel": cancel_label,
            "danger": danger, "alert": alert, "prompt": prompt,
            "value": value, "placeholder": placeholder,
            "extraLabel": extra_label, "detail": detail,
        });
        let json = CString::new(options.to_string()).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&shown);
        window.with_webview(move |webview| unsafe {
            outcome.store(hw_show_dialog(webview.ns_window(), webview.inner(), json.as_ptr()), Ordering::Relaxed);
        }).map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) { return Err("Native dialog is unavailable".into()); }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, dialog_id, title, message, confirm_label, cancel_label, danger, alert, prompt, value, placeholder, extra_label, detail);
        return Err("Native dialog is only available on macOS".into());
    }
    Ok(())
}

#[tauri::command]
pub fn close_macos_dialog(window: tauri::WebviewWindow, dialog_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let dialog_id = CString::new(dialog_id).map_err(|error| error.to_string())?;
        window.with_webview(move |webview| unsafe {
            hw_close_dialog(webview.ns_window(), dialog_id.as_ptr());
        }).map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, dialog_id);
    }
    Ok(())
}

#[tauri::command]
pub fn show_macos_menu(
    window: tauri::WebviewWindow,
    menu_id: String,
    items: Vec<NativeMenuItem>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let options = serde_json::json!({ "menuId": menu_id, "items": items, "x": x, "y": y });
        let json = CString::new(options.to_string()).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&shown);
        window.with_webview(move |webview| unsafe {
            outcome.store(hw_show_menu(webview.ns_window(), webview.inner(), json.as_ptr()), Ordering::Relaxed);
        }).map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) { return Err("Native menu is unavailable".into()); }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, menu_id, items, x, y);
        return Err("Native menu is only available on macOS".into());
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn show_macos_annotation(
    window: tauri::WebviewWindow,
    annotation_id: String,
    title: String,
    comment: String,
    stage_label: String,
    cancel_label: String,
    x: f64,
    y: f64,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let options = serde_json::json!({
            "annotationId": annotation_id, "title": title, "comment": comment,
            "stageLabel": stage_label, "cancelLabel": cancel_label, "x": x, "y": y,
        });
        let json = CString::new(options.to_string()).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&shown);
        window.with_webview(move |webview| unsafe {
            outcome.store(hw_show_annotation(webview.ns_window(), webview.inner(), json.as_ptr()), Ordering::Relaxed);
        }).map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) { return Err("Native annotation is unavailable".into()); }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, annotation_id, title, comment, stage_label, cancel_label, x, y);
        return Err("Native annotation is only available on macOS".into());
    }
    Ok(())
}

#[tauri::command]
pub fn close_macos_annotation(window: tauri::WebviewWindow, annotation_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let annotation_id = CString::new(annotation_id).map_err(|error| error.to_string())?;
        window.with_webview(move |webview| unsafe {
            hw_close_annotation(webview.ns_window(), annotation_id.as_ptr());
        }).map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, annotation_id);
    }
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn show_macos_dropdown(
    window: tauri::WebviewWindow,
    menu_id: String,
    items: Vec<NativeDropdownItem>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    placement: String,
    search_placeholder: Option<String>,
    empty_label: Option<String>,
    close_on_select: bool,
    show_search: Option<bool>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let options = serde_json::json!({
            "menuId": menu_id,
            "items": items,
            "x": x,
            "y": y,
            "width": width,
            "height": height,
            "placement": placement,
            "searchPlaceholder": search_placeholder,
            "emptyLabel": empty_label,
            "closeOnSelect": close_on_select,
            "showSearch": show_search,
        });
        let json = CString::new(options.to_string()).map_err(|error| error.to_string())?;
        let shown = Arc::new(AtomicBool::new(false));
        let outcome = Arc::clone(&shown);
        window
            .with_webview(move |webview| unsafe {
                outcome.store(hw_show_dropdown(webview.ns_window(), webview.inner(), json.as_ptr()), Ordering::Relaxed);
            })
            .map_err(|error| error.to_string())?;
        if !shown.load(Ordering::Relaxed) {
            return Err("Native dropdown is unavailable".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, menu_id, items, x, y, width, height, placement, search_placeholder, empty_label, close_on_select, show_search);
        return Err("Native dropdown is only available on macOS".into());
    }
    Ok(())
}

#[tauri::command]
pub fn close_macos_dropdown(window: tauri::WebviewWindow, menu_id: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let menu_id = CString::new(menu_id).map_err(|error| error.to_string())?;
        window
            .with_webview(move |webview| unsafe { hw_close_dropdown(webview.ns_window(), menu_id.as_ptr()) })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, menu_id);
    }
    Ok(())
}

#[tauri::command]
pub fn update_macos_dropdown(
    window: tauri::WebviewWindow,
    menu_id: String,
    items: Vec<NativeDropdownItem>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let menu_id = CString::new(menu_id).map_err(|error| error.to_string())?;
        let items = CString::new(serde_json::to_string(&items).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        window
            .with_webview(move |webview| unsafe {
                hw_update_dropdown(webview.ns_window(), menu_id.as_ptr(), items.as_ptr());
            })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, menu_id, items);
    }
    Ok(())
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
