pub mod collab_trial;
pub mod curator;
pub mod debug;
pub mod fish;
pub mod gateway;
pub mod scheduler;

use crate::store::{db::ChatMessage, db::Session, db::SessionArtifact, AppState};
use serde::Serialize;
use serde_json::Value;
use tauri::{Emitter, Manager, State};

#[derive(Debug, Serialize)]
pub struct SessionList {
    pub sessions: Vec<Session>,
    pub total: usize,
}

pub(crate) async fn resolve_session_workspace_context(
    state: &State<'_, AppState>,
    session_id: &str,
    default_workspace_root: String,
) -> Result<(String, bool), String> {
    let db = state.db.lock().await;
    let override_root = db
        .get_session(session_id)
        .map_err(|error| error.to_string())?
        .and_then(|session| session.workspace_root)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let has_session_workspace = override_root.is_some();
    Ok((
        override_root.unwrap_or(default_workspace_root),
        has_session_workspace,
    ))
}

pub(crate) async fn resolve_session_workspace_root(
    state: &State<'_, AppState>,
    session_id: &str,
    default_workspace_root: String,
) -> Result<String, String> {
    resolve_session_workspace_context(state, session_id, default_workspace_root)
        .await
        .map(|(workspace_root, _)| workspace_root)
}

#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    title: Option<String>,
    source: Option<String>,
) -> Result<Session, String> {
    let db = state.db.lock().await;
    let source = source
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("chat");
    db.create_session_with_source(title.as_deref(), source)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<SessionList, String> {
    let db = state.db.lock().await;
    let sessions = db
        .list_sessions(limit.unwrap_or(20), offset.unwrap_or(0))
        .map_err(|error| error.to_string())?;
    let total = sessions.len();
    Ok(SessionList { sessions, total })
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let db = state.db.lock().await;
    db.delete_session(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.rename_session(&session_id, &title)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_session_workspace(
    state: State<'_, AppState>,
    session_id: String,
    workspace_root: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.set_session_workspace(&session_id, workspace_root.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_messages(
    state: State<'_, AppState>,
    session_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<ChatMessage>, String> {
    let db = state.db.lock().await;
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);
    if offset == 0 {
        db.get_messages_latest(&session_id, limit)
            .map_err(|error| error.to_string())
    } else {
        db.get_messages_older(&session_id, limit, offset)
            .map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub async fn list_session_artifacts(
    state: State<'_, AppState>,
    session_id: String,
    limit: Option<i64>,
) -> Result<Vec<SessionArtifact>, String> {
    let db = state.db.lock().await;
    db.list_session_artifacts(&session_id, limit.unwrap_or(100).clamp(1, 1000))
        .map_err(|error| error.to_string())
}

fn responses_endpoint(provider: &str, base_url: Option<&str>) -> String {
    let default_base = match provider.to_lowercase().as_str() {
        "deepseek" => "https://api.deepseek.com",
        "openai" => "https://api.openai.com/v1",
        _ => "https://api.openai.com/v1",
    };
    let base = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base)
        .trim_end_matches('/');
    if base.ends_with("/responses") {
        base.to_string()
    } else {
        format!("{base}/responses")
    }
}

pub async fn validate_vision_model(
    provider: &str,
    api_key: &str,
    model: &str,
    base_url: Option<&str>,
) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("API key is required".into());
    }
    const LOGO_BYTES: &[u8] = include_bytes!("../../../public/logo.png");
    use base64::Engine;
    let image_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(LOGO_BYTES)
    );
    let response = reqwest::Client::new()
        .post(responses_endpoint(provider, base_url))
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Describe this image in one word."},
                    {"type": "input_image", "image_url": image_url}
                ]
            }],
            "max_output_tokens": 16
        }))
        .send()
        .await
        .map_err(|error| format!("vision validation request failed: {error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(())
    } else {
        let detail: String = body.chars().take(800).collect();
        Err(format!(
            "Model '{model}' rejected Responses API image input ({status}): {detail}"
        ))
    }
}

pub(crate) fn pool_coordinator_session_id(pool_id: &str) -> String {
    format!("workbuddy_pool_{pool_id}")
}

#[tauri::command]
pub async fn chat_cancel(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    if let Some(flag) = state.cancel_flags.lock().await.get(&session_id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Some(runtime) = state
        .app_handle
        .try_state::<crate::codex_runtime::service::CodexRuntimeState>()
    {
        let _ = runtime.interrupt_session_turn(&session_id).await;
        let _ = runtime.interrupt_headless(&session_id).await;
    }
    {
        let db = state.db.lock().await;
        let _ = db.update_session_status(&session_id, "idle");
    }
    let payload = serde_json::json!({"type": "cancelled"});
    let _ = state
        .app_handle
        .emit(&format!("agent_event_{session_id}"), payload.clone());
    let _ = state.app_handle.emit("agent_broadcast", payload);
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)] // Image is part of the stable frontend preview protocol.
pub enum ContextPreviewBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
        truncated: bool,
    },
    Image {
        note: String,
    },
}

#[derive(Debug, Serialize)]
pub struct ContextPreviewMessage {
    pub role: String,
    pub blocks: Vec<ContextPreviewBlock>,
    pub tokens: usize,
}

#[derive(Debug, Serialize)]
pub struct ContextPreview {
    pub messages: Vec<ContextPreviewMessage>,
    pub messages_tokens: usize,
    pub total_tokens: usize,
    pub request_view_tokens: usize,
    pub idle_indicator_tokens: usize,
    pub model: String,
    pub context_budget: usize,
    pub total_input_budget: usize,
    pub request_overhead_tokens: usize,
    pub tool_count: usize,
    pub rolling_summary_version: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub last_compacted_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4).max(1)
    }
}

fn parse_tool_use_blocks(raw: Option<&str>) -> Vec<ContextPreviewBlock> {
    let Some(raw) = raw else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| {
            let id = value.get("id")?.as_str()?.to_string();
            let name = value.get("name")?.as_str()?.to_string();
            let input = serde_json::to_string_pretty(value.get("input").unwrap_or(&Value::Null))
                .unwrap_or_default();
            Some(ContextPreviewBlock::ToolUse { id, name, input })
        })
        .collect()
}

fn parse_tool_result_blocks(raw: Option<&str>) -> Vec<ContextPreviewBlock> {
    const PREVIEW_LIMIT: usize = 4_000;
    let Some(raw) = raw else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| {
            let tool_use_id = value.get("tool_use_id")?.as_str()?.to_string();
            let raw_content = value
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let truncated = raw_content.chars().count() > PREVIEW_LIMIT;
            let content = if truncated {
                format!(
                    "{}\n\n... [truncated]",
                    raw_content.chars().take(PREVIEW_LIMIT).collect::<String>()
                )
            } else {
                raw_content.to_string()
            };
            Some(ContextPreviewBlock::ToolResult {
                tool_use_id,
                content,
                is_error: value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                truncated,
            })
        })
        .collect()
}

fn preview_message(message: ChatMessage) -> ContextPreviewMessage {
    let mut blocks = Vec::new();
    if !message.content.trim().is_empty() {
        blocks.push(ContextPreviewBlock::Text {
            text: message.content,
        });
    }
    blocks.extend(parse_tool_use_blocks(message.tool_calls_json.as_deref()));
    blocks.extend(parse_tool_result_blocks(
        message.tool_results_json.as_deref(),
    ));
    let tokens = blocks
        .iter()
        .map(|block| match block {
            ContextPreviewBlock::Text { text } => estimate_tokens(text),
            ContextPreviewBlock::ToolUse { input, .. } => estimate_tokens(input),
            ContextPreviewBlock::ToolResult { content, .. } => estimate_tokens(content),
            ContextPreviewBlock::Image { .. } => 85,
        })
        .sum();
    ContextPreviewMessage {
        role: message.role,
        blocks,
        tokens,
    }
}

#[tauri::command]
pub async fn get_context_preview(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ContextPreview, String> {
    let (model, max_tokens, context_window) = {
        let settings = state.settings.lock().await;
        (
            settings.model.clone(),
            settings.max_tokens as usize,
            settings.context_window as usize,
        )
    };
    let (history, context_state) = {
        let db = state.db.lock().await;
        (
            db.get_messages_latest(&session_id, 2_000)
                .map_err(|error| error.to_string())?,
            db.get_session_context_state(&session_id)
                .map_err(|error| error.to_string())?,
        )
    };
    let messages: Vec<_> = history.into_iter().map(preview_message).collect();
    let messages_tokens = messages.iter().map(|message| message.tokens).sum();
    let total_input_budget = context_window.saturating_sub(max_tokens);
    let tool_count = messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter(|block| matches!(block, ContextPreviewBlock::ToolUse { .. }))
        .count();

    Ok(ContextPreview {
        messages,
        messages_tokens,
        total_tokens: messages_tokens,
        request_view_tokens: messages_tokens,
        idle_indicator_tokens: messages_tokens,
        model,
        context_budget: total_input_budget,
        total_input_budget,
        request_overhead_tokens: 0,
        tool_count,
        rolling_summary_version: context_state
            .as_ref()
            .map(|state| state.rolling_summary_version)
            .unwrap_or(0),
        total_input_tokens: context_state
            .as_ref()
            .map(|state| state.total_input_tokens)
            .unwrap_or(0),
        total_output_tokens: context_state
            .as_ref()
            .map(|state| state.total_output_tokens)
            .unwrap_or(0),
        last_compacted_at: context_state.and_then(|state| state.last_compacted_at),
    })
}

#[cfg(test)]
mod tests {
    use super::responses_endpoint;

    #[test]
    fn builds_responses_endpoint() {
        assert_eq!(
            responses_endpoint("deepseek", Some("https://api.deepseek.com")),
            "https://api.deepseek.com/responses"
        );
        assert_eq!(
            responses_endpoint("openai", Some("https://api.openai.com/v1/responses")),
            "https://api.openai.com/v1/responses"
        );
    }
}
