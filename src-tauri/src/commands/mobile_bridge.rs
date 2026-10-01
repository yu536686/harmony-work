//! Windows mobile bridge: account-authenticated pairing, session import, and leased tasks.
use crate::codex_runtime::background::{run_codex_background, CodexBackgroundOptions};
use crate::commands::marketing::{apply_cookie, marketing_client};
use crate::store::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};

const BASE: &str = "https://remote.harmonywatch.cn/api/mobile";
static WORKER: OnceLock<Mutex<Option<(String, tauri::async_runtime::JoinHandle<()>)>>> = OnceLock::new();
static SYNC_CURSOR: OnceLock<Mutex<HashMap<String, (String, String)>>> = OnceLock::new();
static TASK_PERMITS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
static DEVICE_AUTH: OnceLock<Mutex<Option<(String, String)>>> = OnceLock::new();

#[tauri::command]
pub fn mobile_bridge_set_device(device_id: String, device_token: String) -> Result<(), String> {
    uuid(&device_id)?;
    if device_token.len() != 43 || !device_token.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') {
        return Err("Invalid device token".into());
    }
    *DEVICE_AUTH.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "Device auth unavailable")? = Some((device_id, device_token));
    Ok(())
}

fn worker() -> &'static Mutex<Option<(String, tauri::async_runtime::JoinHandle<()>)>> {
    WORKER.get_or_init(|| Mutex::new(None))
}

fn uuid(raw: &str) -> Result<&str, String> {
    uuid::Uuid::parse_str(raw).map_err(|_| "Invalid bridge identifier".to_string())?;
    Ok(raw)
}

async fn request(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let client = marketing_client()?;
    let url = format!("{BASE}{path}");
    let mut builder = apply_cookie(client.request(method, url)).await;
    if let Some((device_id, token)) = DEVICE_AUTH.get_or_init(|| Mutex::new(None))
        .lock().map_err(|_| "Device auth unavailable".to_string())?.clone() {
        builder = builder.header("X-Harmony-Device-Id", device_id)
            .header("X-Harmony-Device-Kind", "desktop")
            .header("X-Harmony-Device-Token", token);
    }
    if let Some(body) = body { builder = builder.json(&body); }
    let response = builder.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let text = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() { return Err(format!("Bridge HTTP {status}: {text}")); }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn mobile_bridge_start_pairing(desktop_name: String) -> Result<Value, String> {
    if desktop_name.trim().is_empty() || desktop_name.len() > 80 { return Err("Invalid desktop name".into()); }
    request(reqwest::Method::POST, "/pairings", Some(json!({"desktopName": desktop_name}))).await
}

#[tauri::command]
pub async fn mobile_bridge_pairing_status(pairing_id: String) -> Result<Value, String> {
    request(reqwest::Method::GET, &format!("/pairings/{}", uuid(&pairing_id)?), None).await
}

#[tauri::command]
pub async fn mobile_bridge_devices() -> Result<Value, String> {
    request(reqwest::Method::GET, "/devices", None).await
}

#[tauri::command]
pub async fn mobile_bridge_signals(device_id: String, after: u64) -> Result<Value, String> {
    request(reqwest::Method::GET, &format!("/signals?deviceId={}&after={after}", uuid(&device_id)?), None).await
}

#[tauri::command]
pub async fn mobile_bridge_send_signal(
    desktop_device_id: String, mobile_device_id: String, sender_device_id: String,
    recipient_device_id: String, kind: String, payload: Value,
) -> Result<Value, String> {
    for value in [&desktop_device_id, &mobile_device_id, &sender_device_id, &recipient_device_id] { uuid(value)?; }
    if !matches!(kind.as_str(), "offer" | "answer" | "candidate") || !payload.is_object() {
        return Err("Invalid signal".into());
    }
    request(reqwest::Method::POST, "/signals", Some(json!({
        "desktopDeviceId": desktop_device_id, "mobileDeviceId": mobile_device_id,
        "senderDeviceId": sender_device_id, "recipientDeviceId": recipient_device_id,
        "kind": kind, "payload": payload,
    }))).await
}

#[tauri::command]
pub async fn mobile_bridge_sync(app: AppHandle, desktop_device_id: String) -> Result<Value, String> {
    let desktop_id = uuid(&desktop_device_id)?;
    let after = SYNC_CURSOR.get_or_init(|| Mutex::new(HashMap::new()))
        .lock().map_err(|_| "Session cursor unavailable".to_string())?
        .get(desktop_id).cloned();
    let state: State<'_, AppState> = app.state();
    let mut imported = 0;
    let mut latest: Option<(String, String)> = after.clone();
    let mut before: Option<(String, String)> = None;
    loop {
        let mut session_path = format!("/sessions?desktopDeviceId={desktop_id}");
        if let Some((timestamp, id)) = &after { session_path.push_str(&format!("&after={timestamp}&afterId={id}")); }
        if let Some((timestamp, id)) = &before { session_path.push_str(&format!("&before={timestamp}&beforeId={id}")); }
        let sessions = request(reqwest::Method::GET, &session_path, None).await?;
        let items = sessions["sessions"].as_array().ok_or("Invalid session page")?;
        let page_size = items.len();
        let db = state.db.lock().await;
        for item in items {
            let Some(id) = item["id"].as_str().and_then(|id| uuid(id).ok()) else { continue };
            let Some(title) = item["title"].as_str() else { continue };
            db.ensure_fixed_session(id, title, "mobile").map_err(|error| error.to_string())?;
            if let Some(messages) = item["messages"].as_array() {
                for message in messages {
                    let (Some(mid), Some(role), Some(content)) = (message["id"].as_str(), message["role"].as_str(), message["content"].as_str()) else { continue };
                    if uuid(mid).is_err() || !matches!(role, "user" | "assistant") || content.len() > 20000 { continue; }
                    if db.import_mobile_message(id, mid, role, content).map_err(|error| error.to_string())? { imported += 1; }
                }
            }
            if let (Some(timestamp), Some(id)) = (item["cursor"].as_str(), item["id"].as_str()) {
                let key = (timestamp.to_string(), id.to_string());
                if latest.as_ref().is_none_or(|current| key > *current) { latest = Some(key); }
            }
        }
        before = items.last().and_then(|item| Some((item["cursor"].as_str()?.to_string(), item["id"].as_str()?.to_string())));
        if page_size < 100 || before.is_none() { break; }
    }
    if let Some(latest) = latest {
        SYNC_CURSOR.get_or_init(|| Mutex::new(HashMap::new()))
            .lock().map_err(|_| "Session cursor unavailable".to_string())?
            .insert(desktop_device_id.clone(), latest);
    }
    let tasks = request(reqwest::Method::GET, &format!("/tasks?desktopDeviceId={desktop_id}&actionable=1"), None).await?;
    let mut claimed = 0;
    if let Some(items) = tasks["tasks"].as_array() {
        for task in items {
            if task["status"] != "queued" && task["status"] != "claimed" { continue; }
            let permit = match TASK_PERMITS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
                .clone().try_acquire_owned() { Ok(permit) => permit, Err(_) => break };
            let Some(task_id) = task["id"].as_str().and_then(|id| uuid(id).ok()) else { continue };
            let claim = request(reqwest::Method::POST, &format!("/tasks/{task_id}/claim"), Some(json!({"desktopDeviceId": desktop_id}))).await;
            let Ok(claim) = claim else { continue; };
            let (Some(lease), Some(session_id), Some(prompt)) = (claim["leaseToken"].as_str(), claim["task"]["sessionId"].as_str(), claim["task"]["prompt"].as_str()) else { continue; };
            if uuid(session_id).is_err() { continue; }
            let (task_id, session_id, prompt, lease) = (task_id.to_owned(), session_id.to_owned(), prompt.to_owned(), lease.to_owned());
            let worker_app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let state: State<'_, AppState> = worker_app.state();
                let options = CodexBackgroundOptions { session_title: Some(prompt.chars().take(40).collect()), session_source: Some("mobile".into()), ..Default::default() };
                let work = run_codex_background(&state, &session_id, &prompt, None, "mobile", options);
                tokio::pin!(work);
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.tick().await;
                let result = loop {
                    tokio::select! {
                        result = &mut work => break result,
                        _ = interval.tick() => {
                            let heartbeat = request(reqwest::Method::POST, &format!("/tasks/{task_id}/heartbeat"), Some(json!({"leaseToken": lease}))).await;
                            if let Err(error) = heartbeat { eprintln!("Mobile bridge heartbeat will retry: {error}"); }
                        }
                    }
                };
                let (status, text) = match result { Ok(output) => ("succeeded", output.final_message), Err(error) => ("failed", error) };
                let completion = json!({"leaseToken": lease, "status": status, "result": text.chars().take(10000).collect::<String>()});
                for attempt in 0..6 {
                    if request(reqwest::Method::POST, &format!("/tasks/{task_id}/complete"), Some(completion.clone())).await.is_ok() { break; }
                    tokio::time::sleep(std::time::Duration::from_secs(2_u64.pow(attempt.min(4)))).await;
                }
            });
            claimed += 1;
        }
    }
    let _ = app.emit("mobile_bridge_updated", ());
    Ok(json!({"importedMessages": imported, "claimedTasks": claimed}))
}

#[tauri::command]
pub async fn mobile_bridge_start_worker(app: AppHandle, desktop_device_id: String) -> Result<(), String> {
    uuid(&desktop_device_id)?;
    let mut active = worker().lock().map_err(|_| "Bridge worker unavailable".to_string())?;
    if active.as_ref().is_some_and(|(id, _)| id == &desktop_device_id) { return Ok(()); }
    if let Some((_, handle)) = active.take() { handle.abort(); }
    let id = desktop_device_id.clone();
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            let _ = mobile_bridge_sync(app.clone(), id.clone()).await;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
    *active = Some((desktop_device_id, handle));
    Ok(())
}

#[tauri::command]
pub fn mobile_bridge_stop_worker() {
    if let Ok(mut active) = worker().lock() {
        if let Some((_, handle)) = active.take() { handle.abort(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_uses_direct_host_and_rejects_non_uuid_ids() {
        assert_eq!(BASE, "https://remote.harmonywatch.cn/api/mobile");
        assert!(uuid("../admin").is_err());
    }
}
