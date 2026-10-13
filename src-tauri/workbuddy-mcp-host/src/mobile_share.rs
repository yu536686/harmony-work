use crate::{tool_success, HostContext};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) fn tool_definition() -> Value {
    json!({
        "name": "share_file",
        "description": "Send an existing local file as an attachment in the current mobile conversation. The desktop validates access and the phone downloads directly while the desktop is online. Submit only a file path and optional display name; the current task identity is supplied by the desktop.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {"type":"string","minLength":1,"description":"Absolute path to an existing local file"},
                "displayName": {"type":"string","minLength":1,"maxLength":120}
            },
            "required": ["path"],
            "additionalProperties": false
        }
    })
}

pub(crate) async fn call(context: &HostContext, arguments: &Value) -> Result<Value> {
    let object = arguments.as_object().ok_or_else(|| anyhow!("share_file expects an object"))?;
    for key in object.keys() {
        if key != "path" && key != "displayName" {
            return Err(anyhow!("share_file does not accept {key}; task identity comes from the desktop"));
        }
    }
    let path = object.get("path").and_then(Value::as_str).map(str::trim)
        .filter(|path| !path.is_empty() && path.len() <= 4096 && Path::new(path).is_absolute())
        .ok_or_else(|| anyhow!("path must be an absolute local file path"))?;
    let display_name = object.get("displayName").and_then(Value::as_str)
        .unwrap_or_else(|| Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(""))
        .trim();
    if display_name.is_empty() || display_name.chars().count() > 120 ||
        display_name.chars().any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control()) {
        return Err(anyhow!("displayName must be a file name without a path"));
    }
    let capability = context.mobile_share_capability.as_deref()
        .ok_or_else(|| anyhow!("share_file is available only for mobile tasks"))?;
    let directory = context.app_data_dir.join("mobile_share_requests").join(capability);
    if !directory.is_dir() { return Err(anyhow!("mobile task is no longer active")); }
    let id = uuid::Uuid::new_v4().to_string();
    let request_path = directory.join(format!("{id}.request.json"));
    let response_path = directory.join(format!("{id}.response.json"));
    let temp_path = directory.join(format!("{id}.tmp"));
    fs::write(&temp_path, serde_json::to_vec(&json!({
        "path": path, "displayName": display_name
    }))?)?;
    fs::rename(&temp_path, &request_path)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(raw) = fs::read(&response_path) {
            let _ = fs::remove_file(&response_path);
            let _ = fs::remove_file(&request_path);
            let response: Value = serde_json::from_slice(&raw)?;
            if let Some(error) = response.get("error").and_then(Value::as_str) {
                return Err(anyhow!("{error}"));
            }
            if response.get("attachmentId").and_then(Value::as_str).is_none() {
                return Err(anyhow!("desktop returned an invalid attachment result"));
            }
            return tool_success(&response);
        }
        if Instant::now() >= deadline || !directory.is_dir() {
            let _ = fs::remove_file(&request_path);
            return Err(anyhow!("desktop did not complete file registration"));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
