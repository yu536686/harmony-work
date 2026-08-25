//! WorkBuddy StaffDeck task dispatcher backed by Codex exec.

use crate::codex_runtime::background::run_codex_transient;
use crate::pool::KoiTodo;
use crate::store::{AppState, Database};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct KoiExecResult {
    pub success: bool,
    pub reply: String,
    pub result_message_id: Option<i64>,
    pub todo: KoiTodo,
}

fn emit(app: &AppHandle, channel: &str, payload: Value) {
    let _ = app.emit(channel, payload);
}

fn emit_todo(app: &AppHandle, action: &str, todo: &KoiTodo) {
    let pool_id = todo.pool_session_id.clone().unwrap_or_default();
    let payload = json!({
        "kind": "todo_changed",
        "pool_id": pool_id,
        "action": action,
        "todo": todo,
    });
    emit(app, "koi_todo_updated", payload.clone());
    emit(app, "host://pool_event", payload);
}

fn emit_koi_status(app: &AppHandle, pool_id: Option<&str>, koi_id: &str, status: &str) {
    let payload = json!({
        "kind": "koi_status_changed",
        "pool_id": pool_id.unwrap_or_default(),
        "koi_id": koi_id,
        "id": koi_id,
        "status": status,
    });
    emit(app, "koi_status_changed", payload.clone());
    emit(app, "host://pool_event", payload);
}

fn emit_pool_message(app: &AppHandle, message: &crate::agent_core::models::PoolMessage) {
    let payload = serde_json::to_value(message).unwrap_or_default();
    emit(
        app,
        &format!("pool_message_{}", message.pool_session_id),
        payload.clone(),
    );
    emit(
        app,
        "host://pool_event",
        json!({
            "kind": "message_appended",
            "pool_id": message.pool_session_id,
            "message": payload,
        }),
    );
}

async fn execute_todo(
    app: &AppHandle,
    state: &AppState,
    todo: KoiTodo,
) -> anyhow::Result<KoiExecResult> {
    let koi = {
        let db = state.db.lock().await;
        db.get_koi(&todo.owner_id)?
            .ok_or_else(|| anyhow::anyhow!("找不到人员：{}", todo.owner_id))?
    };
    let claimed = {
        let db = state.db.lock().await;
        if db.koi_has_other_in_progress_todo(&koi.id, &todo.id)? {
            false
        } else {
            db.try_claim_koi_todo(&todo.id, &koi.id)?
        }
    };
    if !claimed {
        return Ok(KoiExecResult {
            success: false,
            reply: "该人员已有进行中的任务，任务保留在待办队列。".into(),
            result_message_id: None,
            todo,
        });
    }
    {
        let db = state.db.lock().await;
        db.update_koi_status(&koi.id, "busy")?;
    }
    let current = {
        let db = state.db.lock().await;
        db.get_koi_todo(&todo.id)?.unwrap_or_else(|| todo.clone())
    };
    emit_todo(app, "claimed", &current);
    emit_koi_status(app, current.pool_session_id.as_deref(), &koi.id, "busy");

    let prompt = format!(
        "你正在执行 StaffDeck 分配给你的任务。\n\n人员：{}\n角色：{}\n任务：{}\n任务说明：{}\n\n直接完成任务，使用当前工作区和 WorkBuddy MCP 工具。最终回复必须清楚说明完成结果和产物路径。",
        koi.name, koi.role, current.title, current.description
    );
    let context = format!(
        "人员执行规范：\n{}\n\n你的职责描述：{}",
        koi.system_prompt, koi.description
    );
    let outcome = run_codex_transient(state, &prompt, Some(&context)).await;
    match outcome {
        Ok(output) => {
            let result_message_id = if let Some(pool_id) = current.pool_session_id.as_deref() {
                let message = {
                    let db = state.db.lock().await;
                    db.insert_pool_message_ext(
                        pool_id,
                        &koi.id,
                        &output.final_message,
                        "result",
                        &json!({"runtime":"codex_exec","todo_id":current.id}).to_string(),
                        Some(&current.id),
                        None,
                        Some("task_completed"),
                    )?
                };
                let id = message.id;
                emit_pool_message(app, &message);
                Some(id)
            } else {
                None
            };
            let completed = {
                let db = state.db.lock().await;
                db.complete_koi_todo(&current.id, result_message_id)?;
                db.update_koi_status(&koi.id, "idle")?;
                db.get_koi_todo(&current.id)?.unwrap_or(current)
            };
            emit_todo(app, "completed", &completed);
            emit_koi_status(app, completed.pool_session_id.as_deref(), &koi.id, "idle");
            Ok(KoiExecResult {
                success: true,
                reply: output.final_message,
                result_message_id,
                todo: completed,
            })
        }
        Err(error) => {
            let blocked = {
                let db = state.db.lock().await;
                db.block_koi_todo(&current.id, &error)?;
                db.update_koi_status(&koi.id, "error")?;
                db.get_koi_todo(&current.id)?.unwrap_or(current)
            };
            emit_todo(app, "blocked", &blocked);
            emit_koi_status(app, blocked.pool_session_id.as_deref(), &koi.id, "error");
            Ok(KoiExecResult {
                success: false,
                reply: error,
                result_message_id: None,
                todo: blocked,
            })
        }
    }
}

/// Dispatch one StaffDeck task created through the WorkBuddy MCP Host.
pub async fn dispatch_todo_by_id(
    app: &AppHandle,
    state: &AppState,
    todo_id: &str,
) -> anyhow::Result<KoiExecResult> {
    let todo = {
        let db = state.db.lock().await;
        db.get_koi_todo(todo_id)?
            .ok_or_else(|| anyhow::anyhow!("找不到待办：{todo_id}"))?
    };
    execute_todo(app, state, todo).await
}

fn mention_targets(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|part| part.strip_prefix("@!"))
        .map(|value| {
            value
                .trim_matches(|character: char| {
                    matches!(character, ',' | '，' | ':' | '：' | ';' | '；')
                })
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn task_priority(content: &str) -> &str {
    for priority in ["urgent", "high", "medium", "low"] {
        if content.contains(&format!("[Priority: {priority}]")) {
            return priority;
        }
    }
    "medium"
}

pub async fn handle_mention(
    app: &AppHandle,
    state: &AppState,
    sender_id: &str,
    pool_session_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    let targets = mention_targets(content);
    if targets.is_empty() {
        return Ok(());
    }
    for target in targets {
        let todo = {
            let db = state.db.lock().await;
            let koi = db
                .resolve_koi_identifier(&target)?
                .ok_or_else(|| anyhow::anyhow!("找不到人员：{target}"))?;
            db.create_koi_todo(
                &koi.id,
                content,
                content,
                task_priority(content),
                sender_id,
                Some(pool_session_id),
                "staffdeck",
                None,
                0,
            )?
        };
        emit_todo(app, "created", &todo);
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = execute_todo(&app, &state, todo).await {
                tracing::error!(%error, "Codex StaffDeck task failed");
            }
        });
    }
    Ok(())
}

pub async fn handle_mention_arc(
    app: &AppHandle,
    _db: Arc<Mutex<Database>>,
    sender_id: &str,
    pool_session_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow::anyhow!("WorkBuddy state is unavailable"))?;
    handle_mention(app, &state, sender_id, pool_session_id, content).await
}

pub async fn resume_todo(
    app: &AppHandle,
    state: &AppState,
    todo_id: &str,
    _triggered_by: &str,
) -> anyhow::Result<KoiTodo> {
    let todo = {
        let db = state.db.lock().await;
        db.update_koi_todo(todo_id, None, None, Some("todo"), None)?;
        db.get_koi_todo(todo_id)?
            .ok_or_else(|| anyhow::anyhow!("找不到待办：{todo_id}"))?
    };
    let result = execute_todo(app, state, todo).await?;
    Ok(result.todo)
}

#[allow(clippy::too_many_arguments)]
pub async fn replace_todo(
    app: &AppHandle,
    state: &AppState,
    todo_id: &str,
    new_owner_id: &str,
    task: &str,
    reason: &str,
    triggered_by: &str,
    task_timeout_secs: Option<u32>,
) -> anyhow::Result<KoiTodo> {
    let replacement = {
        let db = state.db.lock().await;
        let original = db
            .get_koi_todo(todo_id)?
            .ok_or_else(|| anyhow::anyhow!("找不到待办：{todo_id}"))?;
        db.replace_koi_todo(
            &original,
            new_owner_id,
            task,
            task,
            triggered_by,
            "staffdeck",
            reason,
            task_timeout_secs,
        )?
    };
    emit_todo(app, "created", &replacement);
    let result = execute_todo(app, state, replacement).await?;
    Ok(result.todo)
}

pub async fn activate_pending_todos_arc(
    app: &AppHandle,
    _db: Arc<Mutex<Database>>,
    pool_session_id: Option<&str>,
) -> anyhow::Result<u32> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| anyhow::anyhow!("WorkBuddy state is unavailable"))?;
    activate_pending_todos(app, &state, pool_session_id).await
}

pub async fn activate_pending_todos(
    app: &AppHandle,
    state: &AppState,
    pool_session_id: Option<&str>,
) -> anyhow::Result<u32> {
    let todos = {
        let db = state.db.lock().await;
        db.list_koi_todos(None)?
            .into_iter()
            .filter(|todo| todo.status == "todo")
            .filter(|todo| match pool_session_id {
                Some(pool_id) => todo.pool_session_id.as_deref() == Some(pool_id),
                None => todo.pool_session_id.is_none(),
            })
            .collect::<Vec<_>>()
    };
    let count = todos.len() as u32;
    for todo in todos {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let _ = execute_todo(&app, &state, todo).await;
        });
    }
    Ok(count)
}

pub async fn watchdog_recover(db: Arc<Mutex<Database>>, max_busy_secs: i64) -> (u32, u32) {
    let db = db.lock().await;
    let kois = if max_busy_secs > 0 {
        db.recover_stale_busy_kois(max_busy_secs).unwrap_or(0)
    } else {
        db.recover_stale_koi_status().unwrap_or(0)
    };
    let todos = if max_busy_secs > 0 {
        db.recover_stale_in_progress_todos(max_busy_secs)
            .unwrap_or(0)
    } else {
        db.recover_stale_todos().unwrap_or(0)
    };
    (kois, todos)
}

#[allow(clippy::too_many_arguments)]
pub async fn assign_and_execute(
    app: &AppHandle,
    state: &AppState,
    koi_id: &str,
    task: &str,
    assigned_by: &str,
    priority: &str,
    task_timeout_secs: Option<u32>,
) -> anyhow::Result<KoiExecResult> {
    let todo = {
        let db = state.db.lock().await;
        let koi = db
            .resolve_koi_identifier(koi_id)?
            .ok_or_else(|| anyhow::anyhow!("找不到人员：{koi_id}"))?;
        db.create_koi_todo(
            &koi.id,
            task,
            task,
            priority,
            assigned_by,
            None,
            "staffdeck",
            None,
            task_timeout_secs.unwrap_or(0),
        )?
    };
    emit_todo(app, "created", &todo);
    execute_todo(app, state, todo).await
}

#[cfg(test)]
mod tests {
    use super::{mention_targets, task_priority};

    #[test]
    fn parses_forced_mentions() {
        assert_eq!(mention_targets("@!产品经理 请做方案"), ["产品经理"]);
        assert_eq!(task_priority("[Priority: high] test"), "high");
    }
}
