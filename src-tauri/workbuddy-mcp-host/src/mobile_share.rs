use crate::{tool_success, HostContext};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) fn tool_definition() -> Value {
    json!({
        "name": "share_file",
        "description": "Send an existing local file as an attachment in the current mobile conversation. The desktop validates access and uploads a relay copy over HTTPS for the phone. Submit only a file path and optional display name; the current task identity is supplied by the desktop.",
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
    let object = arguments
        .as_object()
        .ok_or_else(|| anyhow!("share_file expects an object"))?;
    for key in object.keys() {
        if key != "path" && key != "displayName" {
            return Err(anyhow!(
                "share_file does not accept {key}; task identity comes from the desktop"
            ));
        }
    }
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty() && path.len() <= 4096 && Path::new(path).is_absolute())
        .ok_or_else(|| anyhow!("path must be an absolute local file path"))?;
    let display_name = object
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
        })
        .trim();
    if display_name.is_empty()
        || display_name.chars().count() > 120
        || display_name
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control())
    {
        return Err(anyhow!("displayName must be a file name without a path"));
    }
    let parent = context.app_data_dir.join("mobile_share_requests");
    let capability = context
        .mobile_share_capability
        .as_deref()
        .filter(|value| {
            value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        .ok_or_else(|| anyhow!("share_file is available only for active mobile tasks"))?;
    let directory = parent.join(capability);
    if !directory.is_dir() {
        return Err(anyhow!("mobile task is no longer active"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let request_path = directory.join(format!("{id}.request.json"));
    let response_path = directory.join(format!("{id}.response.json"));
    let temp_path = directory.join(format!("{id}.tmp"));
    fs::write(
        &temp_path,
        serde_json::to_vec(&json!({
            "path": path, "displayName": display_name
        }))?,
    )?;
    fs::rename(&temp_path, &request_path)?;
    let deadline = Instant::now() + Duration::from_secs(630);
    loop {
        if let Ok(raw) = fs::read(&response_path) {
            let _ = fs::remove_file(&response_path);
            let _ = fs::remove_file(&request_path);
            let response: Value = serde_json::from_slice(&raw)?;
            if let Some(error) = response.get("error").and_then(Value::as_str) {
                return Err(anyhow!("{error}"));
            }
            if response
                .get("attachmentId")
                .and_then(Value::as_str)
                .is_none()
            {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runtime_regression_global_active_cannot_authorize_ordinary_session() {
        let root = std::env::temp_dir().join(format!("share-isolation-{}", uuid::Uuid::new_v4()));
        let capability = "A".repeat(43);
        let directory = root.join("mobile_share_requests").join(&capability);
        fs::create_dir_all(&directory).unwrap();
        fs::write(root.join("mobile_share_requests/active"), capability).unwrap();
        let context = HostContext::new(
            root.clone(),
            root.join("db"),
            None,
            vec![],
            "http://127.0.0.1:9".into(),
        );
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            call(&context, &json!({"path":root.join("report.txt")})),
        )
        .await;
        let requests = fs::read_dir(&directory).unwrap().count();
        fs::remove_dir_all(root).unwrap();
        assert!(
            result.is_ok_and(|result| result.is_err()),
            "ordinary task must fail immediately even with global active capability"
        );
        assert_eq!(requests, 0);
    }

    #[tokio::test]
    async fn runtime_regression_explicit_task_capability_can_share() {
        let root = std::env::temp_dir().join(format!("share-authorized-{}", uuid::Uuid::new_v4()));
        let capability = "B".repeat(43);
        let directory = root.join("mobile_share_requests").join(&capability);
        fs::create_dir_all(&directory).unwrap();
        let mut context = HostContext::new(
            root.clone(),
            root.join("db"),
            None,
            vec![],
            "http://127.0.0.1:9".into(),
        );
        context.mobile_share_capability = Some(capability);
        let worker = tokio::spawn(async move {
            loop {
                if let Some(path) = fs::read_dir(&directory)
                    .unwrap()
                    .flatten()
                    .map(|entry| entry.path())
                    .find(|path| path.to_string_lossy().ends_with(".request.json"))
                {
                    let response = path.with_file_name(
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .replace(".request.json", ".response.json"),
                    );
                    fs::write(
                        response,
                        json!({"attachmentId":"test-attachment"}).to_string(),
                    )
                    .unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        });
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            call(&context, &json!({"path":root.join("report.txt")})),
        )
        .await
        .unwrap();
        worker.await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok());
    }
}
