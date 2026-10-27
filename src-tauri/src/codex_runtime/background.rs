use crate::codex_runtime::event_adapter::WorkBuddyEvent;
use crate::codex_runtime::headless::{CodexExecOptions, CodexExecResult};
use crate::codex_runtime::runner::{
    harmony_identity_instructions, CodexAttachment, CodexPermissionMode,
};
use crate::codex_runtime::service::{
    permission_mode_from_settings, persist_user_message, persisted_message_context,
    resolve_model_selection, CodexRuntimeState,
};
use crate::gateway::MediaAttachment;
use crate::store::AppState;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

#[derive(Debug, Clone, Default)]
pub struct CodexBackgroundOptions {
    pub session_title: Option<String>,
    pub session_source: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub extra_context: Option<String>,
    pub permission_mode: Option<CodexPermissionMode>,
    pub allow_outside_workspace: Option<bool>,
    pub event_channel: Option<String>,
    pub event_observer: Option<tokio::sync::mpsc::Sender<Value>>,
    pub extra_environment: HashMap<String, String>,
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
    codex
        .configure_credentials_checked(&settings)
        .await
        .map_err(|error| error.to_string())?;
    let (provider, model) = resolve_model_selection(&settings, None, None, None)?;
    let workspace_root = PathBuf::from(&settings.workspace_root);
    std::fs::create_dir_all(&workspace_root).map_err(|error| {
        format!(
            "failed to prepare transient Codex workspace '{}': {error}",
            workspace_root.display()
        )
    })?;
    let transient_id = format!("transient_{}", uuid::Uuid::new_v4());
    let prompt =
        compose_background_prompt(prompt, "internal", &[], extra_context, &provider, &model);
    let result = codex
        .execute_headless(
            &transient_id,
            CodexExecOptions {
                workspace_root,
                provider,
                model,
                prompt,
                permission_mode: CodexPermissionMode::Restricted,
                allow_outside_workspace: false,
                attachments: Vec::new(),
                thread_id: None,
                extra_environment: HashMap::new(),
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
    let event_observer = options.event_observer.clone();
    let codex = state
        .app_handle
        .try_state::<CodexRuntimeState>()
        .ok_or_else(|| "Codex runtime is not initialized".to_string())?;
    codex.attach_usage_database(state.db.clone()).await;
    let settings = state.settings.lock().await.clone();
    codex
        .configure_credentials_checked(&settings)
        .await
        .map_err(|error| error.to_string())?;
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
    let message_context = persisted_message_context(None, None, &attachments);
    let prompt = compose_background_prompt(
        &effective_message,
        channel,
        &prior_messages,
        options.extra_context.as_deref(),
        &provider,
        &model,
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
    }
    crate::commands::usage_policy::check_budget(state, session_id).await?;
    {
        let db = state.db.lock().await;
        let already_inserted = db
            .get_messages_latest(session_id, 1)
            .ok()
            .and_then(|messages| messages.into_iter().last())
            .is_some_and(|message| message.role == "user" && message.content == effective_message);
        if !effective_message.trim().is_empty() || !attachments.is_empty() {
            persist_user_message(
                &db,
                session_id,
                &effective_message,
                message_context.as_deref(),
                already_inserted,
            )?;
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
        .execute_headless_observed(
            session_id,
            CodexExecOptions {
                workspace_root,
                provider,
                model,
                prompt,
                permission_mode,
                allow_outside_workspace: options
                    .allow_outside_workspace
                    .unwrap_or(settings.allow_outside_workspace),
                attachments,
                thread_id: existing_thread,
                extra_environment: options.extra_environment,
            },
            event_observer,
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

pub(crate) fn record_headless_usage(
    db: &crate::store::Database,
    session_id: &str,
    thread_id: &str,
    invocation: &str,
    usage: &crate::codex_runtime::headless::CodexExecUsage,
) -> anyhow::Result<()> {
    use rusqlite::OptionalExtension;
    // One durable invocation watermark makes partial/error/success replay idempotent.
    db.record_codex_usage(session_id, thread_id, 0, 0)?;
    let tx = db.conn.unchecked_transaction()?;
    let key = format!("harmony-exec:{invocation}:{thread_id}");
    let baseline_key = format!("harmony-exec-base:{invocation}:{thread_id}");
    tx.execute("INSERT INTO codex_usage_watermarks (session_id,thread_id,input_tokens,output_tokens)
        SELECT ?1,?2,input_tokens,output_tokens FROM codex_usage_watermarks WHERE session_id=?1 AND thread_id=?3
        ON CONFLICT(session_id,thread_id) DO NOTHING", rusqlite::params![session_id, baseline_key, thread_id])?;
    let baseline: (u32, u32) = tx.query_row(
        "SELECT input_tokens,output_tokens FROM codex_usage_watermarks WHERE session_id=?1 AND thread_id=?2",
        rusqlite::params![session_id, baseline_key], |row| Ok((row.get(0)?,row.get(1)?)),
    )?;
    let previous: (u32, u32) = tx.query_row(
        "SELECT input_tokens, output_tokens FROM codex_usage_watermarks WHERE session_id = ?1 AND thread_id = ?2",
        rusqlite::params![session_id, key], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?.unwrap_or_default();
    let input = usage.input_tokens.min(u64::from(u32::MAX)) as u32;
    let output = usage.output_tokens.min(u64::from(u32::MAX)) as u32;
    let target = (
        baseline.0.saturating_add(input.max(previous.0)),
        baseline.1.saturating_add(output.max(previous.1)),
    );
    let current: (u32,u32) = tx.query_row("SELECT input_tokens,output_tokens FROM codex_usage_watermarks WHERE session_id=?1 AND thread_id=?2", rusqlite::params![session_id,thread_id], |row| Ok((row.get(0)?,row.get(1)?)))?;
    let delta = (
        target.0.saturating_sub(current.0),
        target.1.saturating_sub(current.1),
    );
    tx.execute("UPDATE sessions SET total_input_tokens=total_input_tokens+?1, total_output_tokens=total_output_tokens+?2 WHERE id=?3", rusqlite::params![delta.0, delta.1, session_id])?;
    tx.execute("UPDATE codex_usage_watermarks SET input_tokens=MAX(input_tokens,?1), output_tokens=MAX(output_tokens,?2) WHERE session_id=?3 AND thread_id=?4", rusqlite::params![target.0, target.1, session_id, thread_id])?;
    tx.execute("INSERT INTO codex_usage_watermarks VALUES (?1,?2,?3,?4) ON CONFLICT(session_id,thread_id) DO UPDATE SET input_tokens=MAX(input_tokens,excluded.input_tokens), output_tokens=MAX(output_tokens,excluded.output_tokens)", rusqlite::params![session_id, key, input, output])?;
    tx.commit()?;
    Ok(())
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
    let mut broadcast = payload;
    if let Some(value) = broadcast.as_object_mut() {
        value.insert("sessionId".to_string(), serde_json::json!(session_id));
    }
    let _ = state.app_handle.emit("agent_broadcast", broadcast);
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
    provider: &str,
    model: &str,
) -> String {
    let mut sections = vec![
        format!(
            "<workbuddy_runtime_instructions>\n{}\n</workbuddy_runtime_instructions>",
            harmony_identity_instructions(provider, model)
        ),
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
    let path = directory.join(format!("{}-{filename}", uuid::Uuid::new_v4()));
    std::fs::write(&path, data).map_err(|error| error.to_string())?;
    if message.trim().is_empty() {
        message = "请处理用户通过 IM 发送的附件。".to_string();
    }
    Ok((
        message,
        vec![CodexAttachment {
            path: path.display().to_string(),
            name: filename,
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
    fn runtime_regression_headless_watermark_deduplicates_foreground_catchup_before_result() {
        let db = crate::store::Database::open_in_memory().unwrap();
        db.ensure_fixed_session("s", "usage", "test").unwrap();
        db.record_codex_usage("s", "thread", 100, 10).unwrap();
        record_headless_usage(&db, "s", "thread", "invocation", &Default::default()).unwrap();
        db.record_codex_usage("s", "thread", 150, 15).unwrap();
        let usage = crate::codex_runtime::headless::CodexExecUsage {
            input_tokens: 50,
            output_tokens: 5,
            cached_input_tokens: 0,
        };
        record_headless_usage(&db, "s", "thread", "invocation", &usage).unwrap();
        assert_eq!(
            db.get_session("s").unwrap().unwrap().total_input_tokens,
            150
        );
        assert_eq!(
            db.get_session("s").unwrap().unwrap().total_output_tokens,
            15
        );
    }

    #[test]
    fn runtime_regression_headless_partial_watermark_survives_reopen_and_foreground_replay() {
        let root = std::env::temp_dir().join(format!("exec-watermark-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("usage.db");
        let usage = |input, output| crate::codex_runtime::headless::CodexExecUsage {
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: 0,
        };
        {
            let db = crate::store::Database::open(&path).unwrap();
            db.ensure_fixed_session("s", "usage", "test").unwrap();
            record_headless_usage(&db, "s", "real-thread", "attempt-one", &usage(100, 10)).unwrap();
        }
        let db = crate::store::Database::open(&path).unwrap();
        record_headless_usage(&db, "s", "real-thread", "attempt-one", &usage(100, 10)).unwrap();
        record_headless_usage(&db, "s", "real-thread", "attempt-one", &usage(120, 15)).unwrap();
        record_headless_usage(&db, "s", "real-thread", "attempt-one", &usage(40, 5)).unwrap();
        db.record_codex_usage("s", "real-thread", 120, 15).unwrap();
        record_headless_usage(&db, "s", "real-thread", "attempt-two", &usage(2, 3)).unwrap();
        db.record_codex_usage("s", "real-thread", 122, 18).unwrap();
        let total = db.usage_overview("s", root.to_str().unwrap()).unwrap();
        assert_eq!(total.session_input_tokens, 122);
        assert_eq!(total.session_output_tokens, 18);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_regression_background_exec_usage_accumulates_on_the_real_thread() {
        let db = crate::store::Database::open_in_memory().unwrap();
        let session = db.create_session(None).unwrap();
        let usage = |input, output| crate::codex_runtime::headless::CodexExecUsage {
            input_tokens: input,
            output_tokens: output,
            cached_input_tokens: 0,
        };
        record_headless_usage(&db, &session.id, "real-thread", "one", &usage(100, 10)).unwrap();
        record_headless_usage(&db, &session.id, "real-thread", "one", &usage(100, 10)).unwrap();
        record_headless_usage(&db, &session.id, "real-thread", "two", &usage(40, 5)).unwrap();
        let updated = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(updated.total_input_tokens, 140);
        assert_eq!(updated.total_output_tokens, 15);
        db.record_codex_usage(&session.id, "real-thread", 140, 15)
            .unwrap();
        assert_eq!(
            db.get_session(&session.id)
                .unwrap()
                .unwrap()
                .total_input_tokens,
            140,
            "foreground replay must not recount background usage"
        );
        record_headless_usage(&db, &session.id, "new-real-thread", "three", &usage(2, 3)).unwrap();
        let updated = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(updated.total_input_tokens, 142);
        assert_eq!(updated.total_output_tokens, 18);
    }

    #[test]
    fn first_background_turn_carries_identity_and_migrated_history() {
        let prompt = compose_background_prompt(
            "继续生成报告",
            "wecom",
            &[("user".to_string(), "生成报告".to_string())],
            Some("企业微信入站任务"),
            "harmony",
            "deepseek-v4-flash",
        );
        assert!(prompt.contains("你是小亨，Harmony Work"));
        assert!(prompt.contains("deepseek-v4-flash"));
        assert!(!prompt.contains("Codex app-server"));
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
