//! Windows mobile bridge: account-authenticated pairing, session import, and leased tasks.
use crate::codex_runtime::background::{run_codex_background, CodexBackgroundOptions};
use crate::codex_runtime::service::CodexRuntimeState;
use crate::commands::mobile_task_events::map_codex_progress;
use crate::commands::mobile_attachment::{self, PreparedAttachment, ShareRequest};
use crate::commands::marketing::{apply_cookie, marketing_client};
use crate::store::AppState;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::io::ReaderStream;

const BASE: &str = "https://remote.harmonywatch.cn/api/mobile";

fn attachment_registration_body(lease: &str, file: &PreparedAttachment) -> Value {
    json!({
        "leaseToken": lease, "name": file.name, "mimeType": file.mime_type,
        "size": file.size, "sha256": file.sha256,
    })
}
static WORKER: OnceLock<Mutex<Option<(String, tauri::async_runtime::JoinHandle<()>)>>> = OnceLock::new();
static SYNC_CURSOR: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
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

pub(crate) async fn prepare_desktop_send(
    state: &State<'_, AppState>, session_id: &str, content: &str,
) -> Result<(), String> {
    uuid(session_id)?;
    if content.trim().is_empty() || content.chars().count() > 8_000 {
        return Err("Shared conversation messages must contain 1–8000 characters".into());
    }
    let desktop_id = DEVICE_AUTH.get_or_init(|| Mutex::new(None))
        .lock().map_err(|_| "Device auth unavailable".to_string())?
        .as_ref().map(|(id, _)| id.clone()).ok_or("Please reconnect this desktop")?;
    let message_id = uuid::Uuid::new_v4().to_string();
    let turn_id = uuid::Uuid::new_v4().to_string();
    let started = request(reqwest::Method::POST, &format!("/sessions/{session_id}/turns/start"),
        Some(json!({"turnId":turn_id,"message":{"id":message_id,"content":content}}))).await?;
    let seq = started["message"]["seq"].as_u64().ok_or("Invalid shared message acknowledgement")?;
    let db = state.db.lock().await;
    db.import_shared_message(session_id, &message_id, "user", content, seq, &desktop_id)
        .map_err(|error| error.to_string())?;
    db.record_shared_desktop_turn(session_id, &turn_id, &message_id)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) async fn abort_desktop_send(state: &State<'_, AppState>, session_id: &str) {
    let pending = {
        let db = state.db.lock().await;
        db.pending_shared_desktop_turns().unwrap_or_default()
    };
    for (current_session, turn_id, _) in pending.into_iter().filter(|(id, _, _)| id == session_id) {
        let _ = request(reqwest::Method::POST,
            &format!("/sessions/{current_session}/turns/{turn_id}/complete"),
            Some(json!({"status":"failed"}))).await;
        let db = state.db.lock().await;
        let _ = db.complete_shared_desktop_turn(&turn_id);
    }
}

#[tauri::command]
pub async fn mobile_bridge_active_turn(session_id: String) -> Result<Value, String> {
    let id = uuid(&session_id)?;
    request(reqwest::Method::GET, &format!("/sessions/{id}/turns/active"), None).await
}

#[tauri::command]
pub async fn mobile_bridge_task_events(session_id: String, after_seq: u64) -> Result<Value, String> {
    let id = uuid(&session_id)?;
    request(reqwest::Method::GET, &format!("/sessions/{id}/task-events?afterSeq={after_seq}"), None).await
}

#[tauri::command]
pub async fn mobile_bridge_attachments(session_id: String) -> Result<Value, String> {
    let id = uuid(&session_id)?;
    request(reqwest::Method::GET, &format!("/sessions/{id}/attachments"), None).await
}

#[tauri::command]
pub async fn mobile_bridge_changes(after_seq: u64) -> Result<Value, String> {
    request(reqwest::Method::GET, &format!("/changes?afterSeq={after_seq}"), None).await
}

#[tauri::command]
pub async fn mobile_bridge_tasks() -> Result<Value, String> {
    let id = DEVICE_AUTH.get_or_init(|| Mutex::new(None)).lock()
        .map_err(|_| "Device auth unavailable".to_string())?
        .as_ref().map(|(id, _)| id.clone()).ok_or("Please reconnect this desktop")?;
    request(reqwest::Method::GET, &format!("/tasks?desktopDeviceId={id}"), None).await
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

pub(crate) async fn verify_attachment_ticket(
    attachment_id: &str, ticket: &str, mobile_device_id: &str,
) -> Result<Value, String> {
    uuid(attachment_id)?;
    uuid(mobile_device_id)?;
    if ticket.len() != 43 || !ticket.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') {
        return Err("Invalid transfer ticket".into());
    }
    request(reqwest::Method::POST, &format!("/attachments/{attachment_id}/verify"),
        Some(json!({"ticket":ticket,"mobileDeviceId":mobile_device_id}))).await
}

pub(crate) async fn attachment_status(attachment_id: &str) -> Result<Value, String> {
    uuid(attachment_id)?;
    request(reqwest::Method::GET, &format!("/attachments/{attachment_id}/status"), None).await
}

pub(crate) async fn revoke_attachment(attachment_id: &str) -> Result<(), String> {
    uuid(attachment_id)?;
    request(reqwest::Method::DELETE, &format!("/attachments/{attachment_id}"), None).await.map(|_| ())
}

async fn upload_relay_file(attachment_id: &str, file: &PreparedAttachment) -> Result<(), String> {
    uuid(attachment_id)?;
    let reader = tokio::fs::File::open(&file.path).await.map_err(|_| "Attachment file is unavailable")?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .build().map_err(|error| error.to_string())?;
    let mut builder = apply_cookie(client.put(format!("{BASE}/attachments/{attachment_id}/relay"))).await;
    if let Some((device_id, token)) = DEVICE_AUTH.get_or_init(|| Mutex::new(None))
        .lock().map_err(|_| "Device auth unavailable".to_string())?.clone() {
        builder = builder.header("X-Harmony-Device-Id", device_id)
            .header("X-Harmony-Device-Kind", "desktop")
            .header("X-Harmony-Device-Token", token);
    }
    let response = builder.header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, file.size)
        .body(reqwest::Body::wrap_stream(ReaderStream::new(reader)))
        .send().await.map_err(|error| format!("Relay upload failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Relay upload HTTP {}: {}", response.status(),
            response.text().await.unwrap_or_default().chars().take(200).collect::<String>()));
    }
    Ok(())
}

async fn register_shared_file(
    app: &AppHandle, app_data_dir: PathBuf, task_id: String, lease: String,
    workspace_root: PathBuf, allow_outside_workspace: bool, share: ShareRequest,
) -> Result<Value, String> {
    let file = tokio::task::spawn_blocking(move || {
        mobile_attachment::prepare_file(&share.path, &share.display_name,
            &workspace_root, allow_outside_workspace)
    }).await.map_err(|_| "File inspection stopped unexpectedly".to_string())??;
    let registered = request(reqwest::Method::POST, &format!("/tasks/{task_id}/attachments"),
        Some(attachment_registration_body(&lease, &file))).await?;
    let id = registered["attachment"]["id"].as_str()
        .ok_or_else(|| "Coordinator returned an invalid attachment ID".to_string())?;
    if uuid(id).is_err() { return Err("Coordinator returned an invalid attachment ID".into()); }
    if let Err(error) = mobile_attachment::save_record(&app_data_dir, id, &file) {
        let _ = request(reqwest::Method::DELETE, &format!("/attachments/{id}"), None).await;
        return Err(error);
    }
    if let Err(error) = upload_relay_file(id, &file).await {
        let _ = revoke_attachment(id).await;
        return Err(error);
    }
    let _ = app.emit("mobile_bridge_updated", ());
    Ok(json!({
        "attachmentId": id, "name": file.name, "size": file.size,
        "mimeType": file.mime_type,
    }))
}

async fn run_mobile_share_requests(
    app: AppHandle, app_data_dir: PathBuf, directory: PathBuf,
    task_id: String, lease: String, workspace_root: PathBuf,
    allow_outside_workspace: bool,
) {
    loop {
        match mobile_attachment::take_pending_request(&directory) {
            Ok(Some((processing, share))) => {
                let answer = register_shared_file(&app, app_data_dir.clone(), task_id.clone(),
                    lease.clone(), workspace_root.clone(), allow_outside_workspace, share).await;
                let response = match answer {
                    Ok(value) => value,
                    Err(error) => json!({"error": error.chars().take(240).collect::<String>()}),
                };
                if let Err(error) = mobile_attachment::write_response(&processing, &response) {
                    eprintln!("Mobile file response failed: {error}");
                }
            }
            Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(200)).await,
            Err(_) => break,
        }
    }
}

async fn upload_task_progress(
    app: AppHandle, task_id: String, lease: String,
    mut receiver: tokio::sync::mpsc::Receiver<Value>,
) {
    let mut pending = VecDeque::<Value>::new();
    let mut closed_at: Option<tokio::time::Instant> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            item = receiver.recv(), if closed_at.is_none() => {
                if let Some(raw) = item {
                    if let Some(progress) = map_codex_progress(&raw) {
                        let payload = json!({
                            "leaseToken": lease,
                            "eventId": uuid::Uuid::new_v4().to_string(),
                            "kind": progress.kind,
                            "payload": progress.payload,
                        });
                        let _ = app.emit("mobile_task_changed", json!({
                            "taskId": task_id, "kind": progress.kind, "payload": progress.payload,
                        }));
                        if pending.len() < 128 { pending.push_back(payload); }
                    }
                } else { closed_at = Some(tokio::time::Instant::now()); }
            }
            _ = tick.tick() => {
                if let Some(event) = pending.front() {
                    let sent = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        request(reqwest::Method::POST, &format!("/tasks/{task_id}/events"), Some(event.clone())),
                    ).await.is_ok_and(|result| result.is_ok());
                    if sent { pending.pop_front(); }
                }
            }
        }
        if closed_at.is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(10)) ||
            (closed_at.is_some() && pending.is_empty()) { break; }
    }
}

fn desktop_display_name(value: Option<String>) -> String {
    value.and_then(|name| {
        let clean: String = name.trim().chars().filter(|ch| !ch.is_control()).take(80).collect();
        if clean.is_empty() { None } else { Some(clean) }
    }).unwrap_or_else(|| "Harmony Work Windows".into())
}

#[tauri::command]
pub async fn mobile_bridge_start_pairing() -> Result<Value, String> {
    let desktop_name = desktop_display_name(std::env::var("COMPUTERNAME").ok());
    let existing = DEVICE_AUTH.get_or_init(|| Mutex::new(None)).lock()
        .map_err(|_| "Device auth unavailable".to_string())?.clone();
    if let Some((device_id, _)) = existing {
        let reused = request(reqwest::Method::POST, "/pairings",
            Some(json!({"desktopName": desktop_name, "desktopDeviceId": device_id}))).await;
        match reused {
            Ok(pairing) => return Ok(pairing),
            Err(error) if error.contains("Bridge HTTP 403") => {
                *DEVICE_AUTH.get_or_init(|| Mutex::new(None)).lock()
                    .map_err(|_| "Device auth unavailable".to_string())? = None;
            }
            Err(error) => return Err(error),
        }
    }
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
pub async fn mobile_bridge_unlink(app: AppHandle, desktop_device_id: String, mobile_device_id: String) -> Result<Value, String> {
    let desktop_id = uuid(&desktop_device_id)?;
    let mobile_id = uuid(&mobile_device_id)?;
    let result = request(reqwest::Method::DELETE, &format!("/links/{desktop_id}/{mobile_id}"), None).await?;
    let _ = app.emit("mobile_bridge_link_changed", ());
    let _ = app.emit("mobile_bridge_updated", ());
    Ok(result)
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
    let state: State<'_, AppState> = app.state();
    let mut imported = 0;
    let mut uploaded = 0;
    let mut before: Option<(String, String)> = None;
    loop {
        let mut session_path = format!("/sessions?desktopDeviceId={desktop_id}");
        if let Some((timestamp, id)) = &before {
            session_path.push_str(&format!("&before={timestamp}&beforeId={id}"));
        }
        let sessions = request(reqwest::Method::GET, &session_path, None).await?;
        let items = sessions["sessions"].as_array().ok_or("Invalid session page")?;
        let page_size = items.len();
        for item in items {
            let Some(id) = item["id"].as_str().and_then(|id| uuid(id).ok()) else { continue };
            let Some(title) = item["title"].as_str() else { continue };
            {
                let db = state.db.lock().await;
                let local = db.ensure_fixed_session(id, title, "mobile").map_err(|error| error.to_string())?;
                if local.title.as_deref() != Some(title) {
                    db.rename_session(id, title).map_err(|error| error.to_string())?;
                }
            }
            let mut after_seq = SYNC_CURSOR.get_or_init(|| Mutex::new(HashMap::new()))
                .lock().map_err(|_| "Session cursor unavailable".to_string())?
                .get(id).copied().unwrap_or(0);
            loop {
                let page = request(reqwest::Method::GET,
                    &format!("/sessions/{id}/messages?afterSeq={after_seq}"), None).await?;
                let messages = page["messages"].as_array().ok_or("Invalid message page")?;
                if messages.is_empty() { break; }
                let previous_seq = after_seq;
                for message in messages {
                    let (Some(mid), Some(role), Some(content), Some(seq)) = (
                        message["id"].as_str(), message["role"].as_str(),
                        message["content"].as_str(), message["seq"].as_u64()) else { continue };
                    if uuid(mid).is_err() || !matches!(role, "user" | "assistant") { continue; }
                    let origin = message["originDeviceId"].as_str().unwrap_or("");
                    let db = state.db.lock().await;
                    if db.import_shared_message(id, mid, role, content, seq, origin)
                        .map_err(|error| error.to_string())? { imported += 1; }
                    after_seq = seq;
                }
                SYNC_CURSOR.get_or_init(|| Mutex::new(HashMap::new()))
                    .lock().map_err(|_| "Session cursor unavailable".to_string())?
                    .insert(id.to_string(), after_seq);
                if messages.len() < 100 || after_seq == previous_seq { break; }
            }
            let unshared = {
                let db = state.db.lock().await;
                db.get_unshared_mobile_messages(id, 100).map_err(|error| error.to_string())?
            };
            for message in unshared {
                if !matches!(message.role.as_str(), "user" | "assistant") || message.content.chars().count() > 8_000 { continue; }
                let ack = request(reqwest::Method::POST, &format!("/sessions/{id}/messages"),
                    Some(json!({"id":message.id,"role":message.role,"content":message.content}))).await;
                let Ok(ack) = ack else { continue; };
                let Some(seq) = ack["message"]["seq"].as_u64() else { continue; };
                let db = state.db.lock().await;
                db.import_shared_message(id, &message.id, &message.role, &message.content, seq, desktop_id)
                    .map_err(|error| error.to_string())?;
                uploaded += 1;
            }
        }
        before = items.last().and_then(|item| Some((item["cursor"].as_str()?.to_string(), item["id"].as_str()?.to_string())));
        if page_size < 100 || before.is_none() { break; }
    }
    let pending_turns = {
        let db = state.db.lock().await;
        db.pending_shared_desktop_turns().map_err(|error| error.to_string())?
    };
    for (session_id, turn_id, initiating_message_id) in pending_turns {
        let active = request(reqwest::Method::GET,
            &format!("/sessions/{session_id}/turns/active"), None).await;
        let Ok(active) = active else { continue; };
        if active["turn"]["id"].as_str() != Some(turn_id.as_str()) { continue; }
        let completed = {
            let db = state.db.lock().await;
            db.has_shared_assistant_after(&session_id, &initiating_message_id)
                .map_err(|error| error.to_string())?
        };
        let path = if completed { "complete" } else { "heartbeat" };
        let body = if completed { json!({"status":"completed"}) } else { json!({}) };
        if request(reqwest::Method::POST,
            &format!("/sessions/{session_id}/turns/{turn_id}/{path}"), Some(body)).await.is_ok() && completed {
            let db = state.db.lock().await;
            db.complete_shared_desktop_turn(&turn_id).map_err(|error| error.to_string())?;
        }
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
            let (task_id, prompt, lease) = (task_id.to_owned(), prompt.to_owned(), lease.to_owned());
            let worker_app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let state: State<'_, AppState> = worker_app.state();
                let file_settings = state.settings.lock().await.clone();
                let workspace_root = PathBuf::from(&file_settings.workspace_root);
                let app_data_dir = worker_app.path().app_data_dir().ok();
                let share_context = app_data_dir.as_ref().and_then(|directory| {
                    mobile_attachment::create_request_context(directory).ok()
                });
                let mut extra_environment = HashMap::new();
                let share_worker = match (app_data_dir, share_context) {
                    (Some(app_data_dir), Some((capability, directory))) => {
                        extra_environment.insert("WORKBUDDY_MOBILE_TASK_CAPABILITY".into(), capability);
                        let handle = tauri::async_runtime::spawn(run_mobile_share_requests(
                            worker_app.clone(), app_data_dir, directory.clone(), task_id.clone(),
                            lease.clone(), workspace_root.clone(), file_settings.allow_outside_workspace,
                        ));
                        Some((handle, directory))
                    }
                    _ => None,
                };
                let (observer, progress) = tokio::sync::mpsc::channel(64);
                let uploader = tauri::async_runtime::spawn(upload_task_progress(
                    worker_app.clone(), task_id.clone(), lease.clone(), progress,
                ));
                let options = CodexBackgroundOptions {
                    session_title: Some(prompt.chars().take(40).collect()),
                    session_source: Some("mobile_executor".into()),
                    workspace_root: Some(workspace_root),
                    extra_context: share_worker.as_ref().map(|_| {
                        "当用户需要获取电脑中的文件时，使用 share_file 工具将文件作为附件发到当前手机会话；工具只需要绝对路径和显示名。".into()
                    }),
                    extra_environment,
                    event_observer: Some(observer),
                    ..Default::default()
                };
                let work = run_codex_background(&state, &task_id, &prompt, None, "mobile", options);
                tokio::pin!(work);
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                interval.tick().await;
                let mut lease_lost = false;
                let result = loop {
                    tokio::select! {
                        result = &mut work => break result,
                        _ = interval.tick() => {
                            let heartbeat = request(reqwest::Method::POST, &format!("/tasks/{task_id}/heartbeat"), Some(json!({"leaseToken": lease}))).await;
                            if let Err(error) = heartbeat {
                                if error.contains("409") || error.contains("403") {
                                    lease_lost = true;
                                    if let Some(runtime) = worker_app.try_state::<CodexRuntimeState>() {
                                        runtime.interrupt_headless(&task_id).await;
                                    }
                                } else { eprintln!("Mobile bridge heartbeat will retry: {error}"); }
                            }
                        }
                    }
                };
                if let Some((handle, directory)) = share_worker {
                    handle.abort();
                    mobile_attachment::clear_request_context(&directory);
                }
                if lease_lost { uploader.abort(); return; }
                let _ = tokio::time::timeout(std::time::Duration::from_secs(11), uploader).await;
                let (status, text) = match result {
                    Ok(output) if approval_service_unavailable(&output.final_message, &output.events) =>
                        ("failed", output.final_message),
                    Ok(output) => ("succeeded", output.final_message),
                    Err(error) => ("failed", error),
                };
                let completion = json!({"leaseToken": lease, "status": status, "result": text.chars().take(10000).collect::<String>()});
                for attempt in 0..6 {
                    if request(reqwest::Method::POST, &format!("/tasks/{task_id}/complete"), Some(completion.clone())).await.is_ok() {
                        let _ = worker_app.emit("mobile_task_changed", json!({"taskId":task_id,"kind":status}));
                        let _ = worker_app.emit("mobile_bridge_updated", ());
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2_u64.pow(attempt.min(4)))).await;
                }
            });
            claimed += 1;
        }
    }
    if imported > 0 || uploaded > 0 || claimed > 0 { let _ = app.emit("mobile_bridge_updated", ()); }
    Ok(json!({"importedMessages": imported, "uploadedMessages": uploaded, "claimedTasks": claimed}))
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
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
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

fn approval_service_unavailable(final_message: &str, events: &[Value]) -> bool {
    let mentions_outage = |text: &str| {
        (text.contains("codex-auto-review") &&
            (text.contains("503") || text.contains("No available channel"))) ||
        text.contains("No available channel for model codex-auto-review")
    };
    mentions_outage(final_message) || events.iter().any(|event| mentions_outage(&event.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_uses_direct_host_and_rejects_non_uuid_ids() {
        assert_eq!(BASE, "https://remote.harmonywatch.cn/api/mobile");
        assert!(uuid("../admin").is_err());
    }

    #[test]
    fn windows_computer_name_is_bounded_with_fallback() {
        assert_eq!(desktop_display_name(Some("  WORKSTATION-01  ".into())), "WORKSTATION-01");
        assert_eq!(desktop_display_name(Some(" ".into())), "Harmony Work Windows");
        assert_eq!(desktop_display_name(Some("A".repeat(100))).len(), 80);
    }

    #[test]
    fn approval_outage_does_not_report_a_completed_desktop_task() {
        assert!(approval_service_unavailable(
            "工具调用失败：codex-auto-review 返回 503 Service Unavailable",
            &[]
        ));
        assert!(approval_service_unavailable(
            "我无法读取电脑信息。",
            &[json!({"type":"item.completed","item":{"error":"No available channel for model codex-auto-review under group harmony"}})]
        ));
        assert!(!approval_service_unavailable("已读取电脑上的版本号。", &[]));
    }

    #[test]
    fn mobile_attachment_registration_contains_metadata_but_no_local_path_or_model_identity() {
        let file = PreparedAttachment {
            path: std::path::PathBuf::from("C:\\private\\report.txt"),
            name: "report.txt".into(), mime_type: "text/plain".into(),
            size: 4, sha256: "a".repeat(64), modified_ns: 42,
        };
        let body = attachment_registration_body("trusted-lease", &file);
        assert_eq!(body["leaseToken"], "trusted-lease");
        assert_eq!(body["name"], "report.txt");
        assert_eq!(body["size"], 4);
        assert!(body.get("path").is_none());
        assert!(body.get("taskId").is_none());
        assert!(body.get("sessionId").is_none());
        assert!(body.get("modifiedNs").is_none());
    }
}
