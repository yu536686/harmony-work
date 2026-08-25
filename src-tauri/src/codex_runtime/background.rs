use crate::codex_runtime::event_adapter::WorkBuddyEvent;
use crate::codex_runtime::headless::{CodexExecOptions, CodexExecResult};
use crate::codex_runtime::runner::{CodexAttachment, CodexPermissionMode};
use crate::codex_runtime::service::{
    permission_mode_from_settings, resolve_model_selection, CodexRuntimeState,
};
use crate::gateway::MediaAttachment;
use crate::store::AppState;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, Default)]
pub struct CodexBackgroundOptions {
    pub session_title: Option<String>,
    pub session_source: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub extra_context: Option<String>,
    pub permission_mode: Option<CodexPermissionMode>,
    pub event_channel: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexBackgroundOutput {
    pub final_message: String,
    pub thread_id: Option<String>,
    pub usage: crate::codex_runtime::headless::CodexExecUsage,
    pub events: Vec<Value>,
}

/// Run a one-shot Codex prompt without creating a WorkBuddy chat session.
///
/// This is used by internal assistants such as skill curation and prompt
/// composition. Codex still creates an ephemeral thread internally, so remove
/// its WorkBuddy mapping as soon as the turn finishes.
pub async fn run_codex_transient(
    state: &AppState,
    prompt: &str,
    extra_context: Option<&str>,
) -> Result<CodexBackgroundOutput, String> {
    let codex = state
        .app_handle
        .try_state::<CodexRuntimeState>()
        .ok_or_else(|| "Codex runtime is not initialized".to_string())?;
    let settings = state.settings.lock().await.clone();
    codex.configure_credentials(&settings).await;
    let (provider, model) = resolve_model_selection(&settings, None, None, None)?;
    let workspace_root = PathBuf::from(&settings.workspace_root);
    std::fs::create_dir_all(&workspace_root).map_err(|error| {
        format!(
            "failed to prepare transient Codex workspace '{}': {error}",
            workspace_root.display()
        )
    })?;
    let transient_id = format!("transient_{}", uuid::Uuid::new_v4());
    let result = codex
        .execute_headless(
            &transient_id,
            CodexExecOptions {
                workspace_root,
                provider,
                model,
                prompt: compose_background_prompt(prompt, "internal", &[], extra_context),
                permission_mode: CodexPermissionMode::Restricted,
                attachments: Vec::new(),
                thread_id: None,
            },
        )
        .await;
    let _ = codex.sessions().remove(&transient_id);
    let result = result.map_err(|error| error.to_string())?;
    let final_message = result.final_message.trim().to_string();
    if final_message.is_empty() {
        return Err("Codex did not return text content".to_string());
    }
    Ok(CodexBackgroundOutput {
        final_message,
        thread_id: result.thread_id,
        usage: result.usage,
        events: result.events,
    })
}

pub async fn run_codex_background(
    state: &AppState,
    session_id: &str,
    user_message: &str,
    inbound_media: Option<MediaAttachment>,
    channel: &str,
    options: CodexBackgroundOptions,
) -> Result<CodexBackgroundOutput, String> {
    let event_channel = options.event_channel.clone();
    let codex = state
        .app_handle
        .try_state::<CodexRuntimeState>()
        .ok_or_else(|| "Codex runtime is not initialized".to_string())?;
    let settings = state.settings.lock().await.clone();
    codex.configure_credentials(&settings).await;
    let (provider, model) = resolve_model_selection(&settings, None, None, None)?;
    let workspace_root = options
        .workspace_root
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from(&settings.workspace_root));
    std::fs::create_dir_all(&workspace_root).map_err(|error| {
        format!(
            "failed to prepare background workspace '{}': {error}",
            workspace_root.display()
        )
    })?;
    let existing_thread = codex
        .sessions()
        .get(session_id)
        .map_err(|error| error.to_string())?
        .map(|binding| binding.codex_thread_id);
    let prior_messages = if existing_thread.is_none() {
        let db = state.db.lock().await;
        db.get_messages_latest(session_id, 12)
            .unwrap_or_default()
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect()
    } else {
        Vec::new()
    };
    let (effective_message, attachments) =
        materialize_inbound_media(&state.app_handle, session_id, user_message, inbound_media)?;
    let prompt = compose_background_prompt(
        &effective_message,
        channel,
        &prior_messages,
        options.extra_context.as_deref(),
    );
    let title = options
        .session_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(session_id);
    let source = options
        .session_source
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .unwrap_or(channel);
    {
        let db = state.db.lock().await;
        db.ensure_fixed_session(session_id, title, source)
            .map_err(|error| error.to_string())?;
        let already_inserted = db
            .get_messages_latest(session_id, 1)
            .ok()
            .and_then(|messages| messages.into_iter().last())
            .is_some_and(|message| message.role == "user" && message.content == effective_message);
        if !already_inserted && !effective_message.trim().is_empty() {
            db.append_message(session_id, "user", &effective_message)
                .map_err(|error| error.to_string())?;
        }
        db.update_session_status(session_id, "running")
            .map_err(|error| error.to_string())?;
    }
    let permission_mode = options.permission_mode.unwrap_or_else(|| {
        permission_mode_from_settings(
            settings.confirm_shell_commands,
            settings.confirm_file_writes,
        )
    });
    let result = codex
        .execute_headless(
            session_id,
            CodexExecOptions {
                workspace_root,
                provider,
                model,
                prompt,
                permission_mode,
                attachments,
                thread_id: existing_thread,
            },
        )
        .await;
    match result {
        Ok(result) => finalize_success(state, session_id, result, event_channel.as_deref()).await,
        Err(error) => {
            let message = error.to_string();
            {
                let db = state.db.lock().await;
                let _ = db.update_session_status(session_id, "idle");
            }
            emit_event(
                state,
                session_id,
                event_channel.as_deref(),
                WorkBuddyEvent::Error {
                    message: message.clone(),
                },
            );
            let _ = state.app_handle.emit("im_session_done", session_id);
            Err(message)
        }
    }
}

async fn finalize_success(
    state: &AppState,
    session_id: &str,
    result: CodexExecResult,
    event_channel: Option<&str>,
) -> Result<CodexBackgroundOutput, String> {
    for event in &result.events {
        for mapped in map_exec_event(event, &result) {
            emit_event(state, session_id, event_channel, mapped);
        }
    }
    let final_message = if result.final_message.trim().is_empty() {
        "（Codex 未返回文本内容）".to_string()
    } else {
        result.final_message.clone()
    };
    {
        let db = state.db.lock().await;
        db.append_message(session_id, "assistant", &final_message)
            .map_err(|error| error.to_string())?;
        db.update_session_status(session_id, "idle")
            .map_err(|error| error.to_string())?;
    }
    let _ = state.app_handle.emit("im_session_done", session_id);
    Ok(CodexBackgroundOutput {
        final_message,
        thread_id: result.thread_id,
        usage: result.usage,
        events: result.events,
    })
}

fn emit_event(
    state: &AppState,
    session_id: &str,
    event_channel: Option<&str>,
    event: WorkBuddyEvent,
) {
    let payload = serde_json::to_value(event).unwrap_or_default();
    let _ = state
        .app_handle
        .emit(&format!("agent_event_{session_id}"), payload.clone());
    if let Some(event_channel) = event_channel {
        let _ = state.app_handle.emit(event_channel, payload.clone());
    }
    let _ = state.app_handle.emit("agent_broadcast", payload);
}

fn map_exec_event(event: &Value, result: &CodexExecResult) -> Vec<WorkBuddyEvent> {
    match event.get("type").and_then(Value::as_str) {
        Some("turn.started") => vec![
            WorkBuddyEvent::RunStatus {
                iteration: 1,
                phase: "thinking".to_string(),
                detail: "正在处理任务".to_string(),
            },
            WorkBuddyEvent::TextSegmentStart { iteration: 1 },
        ],
        Some("item.started") => event
            .get("item")
            .and_then(exec_tool_identity)
            .map(|(id, name)| {
                vec![WorkBuddyEvent::ToolStart {
                    id,
                    name,
                    input: event.get("item").cloned().unwrap_or_default(),
                }]
            })
            .unwrap_or_default(),
        Some("item.completed") => event
            .get("item")
            .and_then(exec_tool_identity)
            .map(|(id, name)| {
                let item = event.get("item").cloned().unwrap_or_default();
                let is_error = matches!(
                    item.get("status").and_then(Value::as_str),
                    Some("failed" | "declined")
                ) || item
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .is_some_and(|code| code != 0);
                vec![WorkBuddyEvent::ToolEnd {
                    id,
                    name,
                    result: item
                        .get("aggregated_output")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| item.to_string()),
                    is_error,
                }]
            })
            .unwrap_or_default(),
        Some("turn.completed") => vec![
            WorkBuddyEvent::RunStatus {
                iteration: 1,
                phase: "closing".to_string(),
                detail: "任务已完成".to_string(),
            },
            WorkBuddyEvent::Done {
                total_input_tokens: result.usage.input_tokens.min(u32::MAX as u64) as u32,
                total_output_tokens: result.usage.output_tokens.min(u32::MAX as u64) as u32,
            },
        ],
        Some("error") => vec![WorkBuddyEvent::Error {
            message: event
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex exec 执行失败")
                .to_string(),
        }],
        _ => Vec::new(),
    }
}

fn exec_tool_identity(item: &Value) -> Option<(String, String)> {
    let id = item.get("id")?.as_str()?.to_string();
    let name = match item.get("type")?.as_str()? {
        "command_execution" => "shell".to_string(),
        "file_change" => "apply_patch".to_string(),
        "mcp_tool_call" => item
            .get("tool")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("mcp_tool")
            .to_string(),
        "web_search" => "web_search".to_string(),
        _ => return None,
    };
    Some((id, name))
}

fn compose_background_prompt(
    user_message: &str,
    channel: &str,
    prior_messages: &[(String, String)],
    extra_context: Option<&str>,
) -> String {
    let mut sections = vec![
        "<workbuddy_runtime_instructions>\n你是小亨 WorkBuddy，是用户的桌面 AI 工作伙伴。始终自称“小亨”，直接完成任务。使用 Codex 原生工具和 WorkBuddy MCP 能力，不要声称自己是 Piscis，也不要回退到旧 Agent Loop。\n</workbuddy_runtime_instructions>".to_string(),
        format!("<workbuddy_channel>{channel}</workbuddy_channel>"),
    ];
    if !prior_messages.is_empty() {
        let history = prior_messages
            .iter()
            .map(|(role, content)| {
                let content = content.chars().take(2_000).collect::<String>();
                format!("{role}: {content}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "<workbuddy_migrated_history>\n{history}\n</workbuddy_migrated_history>"
        ));
    }
    if let Some(extra_context) = extra_context
        .map(str::trim)
        .filter(|context| !context.is_empty())
    {
        sections.push(format!(
            "<workbuddy_background_context>\n{extra_context}\n</workbuddy_background_context>"
        ));
    }
    sections.push(user_message.to_string());
    sections.join("\n\n")
}

fn materialize_inbound_media(
    app: &tauri::AppHandle,
    session_id: &str,
    user_message: &str,
    media: Option<MediaAttachment>,
) -> Result<(String, Vec<CodexAttachment>), String> {
    let Some(media) = media else {
        return Ok((user_message.to_string(), Vec::new()));
    };
    let mut message = user_message.to_string();
    let Some(data) = media.data else {
        message.push_str(&format!(
            "\n\n[IM 附件线索：类型={}，文件名={:?}，URL={:?}]",
            media.media_type, media.filename, media.url
        ));
        return Ok((message, Vec::new()));
    };
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let directory = app_data
        .join("inbound-media")
        .join(sanitize_path_segment(session_id));
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let filename = media
        .filename
        .as_deref()
        .and_then(|name| Path::new(name).file_name())
        .and_then(|name| name.to_str())
        .map(sanitize_path_segment)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| default_media_filename(&media.media_type));
    let path = directory.join(filename);
    std::fs::write(&path, data).map_err(|error| error.to_string())?;
    if message.trim().is_empty() {
        message = "请处理用户通过 IM 发送的附件。".to_string();
    }
    Ok((
        message,
        vec![CodexAttachment {
            path: path.display().to_string(),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .to_string(),
            media_type: media.media_type,
        }],
    ))
}

fn default_media_filename(media_type: &str) -> String {
    let extension = match media_type {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "video/mp4" => "mp4",
        _ => "bin",
    };
    format!("attachment-{id}.{extension}", id = uuid::Uuid::new_v4())
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_background_turn_carries_identity_and_migrated_history() {
        let prompt = compose_background_prompt(
            "继续生成报告",
            "wecom",
            &[("user".to_string(), "生成报告".to_string())],
            Some("企业微信入站任务"),
        );
        assert!(prompt.contains("你是小亨 WorkBuddy"));
        assert!(prompt.contains("user: 生成报告"));
        assert!(prompt.contains("企业微信入站任务"));
        assert!(prompt.ends_with("继续生成报告"));
    }

    #[test]
    fn maps_exec_tool_events_to_existing_activity_contract() {
        let result = CodexExecResult {
            thread_id: Some("thread-1".to_string()),
            final_message: "完成".to_string(),
            usage: Default::default(),
            events: Vec::new(),
            stderr: String::new(),
        };
        let events = map_exec_event(
            &serde_json::json!({
                "type":"item.completed",
                "item":{"id":"cmd-1","type":"command_execution","status":"completed","aggregated_output":"ok","exit_code":0}
            }),
            &result,
        );
        assert!(matches!(
            events.as_slice(),
            [WorkBuddyEvent::ToolEnd { name, is_error: false, .. }] if name == "shell"
        ));
    }
}
