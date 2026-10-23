use crate::codex_runtime::background::{run_codex_background, CodexBackgroundOptions};
use crate::store::{db::ScheduledTask, AppState, Database};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;
use tracing::{info, warn};

const MEMORY_CONSOLIDATION_TASK_NAME: &str = "Memory Consolidation";
const MEMORY_CONSOLIDATION_MARKER: &str = "[template:memory_consolidation]";
const MEMORY_CONSOLIDATION_DEFAULT_CRON: &str = "0 4 * * *";
const CONSOLIDATION_MAX_AGE_DAYS: i64 = 30;

fn recent_for_consolidation(updated_at: DateTime<Utc>) -> bool {
    let age = Utc::now().signed_duration_since(updated_at);
    age >= Duration::zero() && age <= Duration::days(CONSOLIDATION_MAX_AGE_DAYS)
}

#[derive(Debug, Serialize)]
pub struct TaskList {
    pub tasks: Vec<ScheduledTask>,
    pub total: usize,
}

fn trim_preview(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(limit).collect::<String>())
    }
}

fn build_scheduler_session_title(task_name: Option<&str>, task_id: &str) -> String {
    let title = task_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let short_id = task_id.chars().take(8).collect::<String>();
            short_id
        });
    format!("[定时] {}", title)
}

fn build_memory_consolidation_template() -> String {
    format!(
        "{marker}\n\
         You are running WorkBuddy's background memory consolidation pass.\n\
         Review the supplied snapshot of recent sessions, task spines, and stored memories.\n\
         Your job is to preserve only high-signal, durable information.\n\n\
         Rules:\n\
         1. Prefer facts, decisions, procedures, user preferences, and durable project context.\n\
         2. Do not restate routine chatter or ephemeral intermediate steps.\n\
         3. Before saving, call the WorkBuddy MCP memory_search tool to check for duplicates.\n\
         4. Save at most 3-5 high-value memories with the WorkBuddy MCP memory_append tool.\n\
            Do not write a separate memory file or claim a save without a successful tool result.\n\
            Do not store credentials, private tokens, or transient task progress.\n\
         5. Only discuss tasks tied to recent sessions in the supplied snapshot.\n\
         6. Keep the final reply concise: what you consolidated, what you skipped, and any recall/reminder worth showing.\n",
        marker = MEMORY_CONSOLIDATION_MARKER
    )
}

fn is_memory_consolidation_prompt(task_prompt: &str) -> bool {
    task_prompt.contains(MEMORY_CONSOLIDATION_MARKER)
}

fn memory_revision(db: &Database) -> Vec<(String, String, f64, Option<String>)> {
    db.list_memories_for_owner("xiaoheng")
        .unwrap_or_default()
        .into_iter()
        .map(|memory| {
            (
                memory.id,
                memory.content,
                memory.confidence,
                memory.last_seen_at.map(|time| time.to_rfc3339()),
            )
        })
        .collect()
}

fn build_memory_consolidation_snapshot(db: &Database) -> String {
    let sessions = db.list_sessions(200, 0, false).unwrap_or_default();
    let task_states = db.list_recent_task_states(100).unwrap_or_default();
    let memories = db.list_memories_for_owner("xiaoheng").unwrap_or_default();

    let recent_sessions = sessions.iter()
        .filter(|s| s.source != "scheduled_task" && recent_for_consolidation(s.updated_at))
        .take(6).collect::<Vec<_>>();
    let session_lines = if recent_sessions.is_empty() {
        "- No recent sessions".to_string()
    } else {
        recent_sessions
            .iter()
            .map(|s| {
                let summary = if s.rolling_summary.trim().is_empty() {
                    "no rolling summary".to_string()
                } else {
                    trim_preview(&s.rolling_summary.replace('\n', " "), 180)
                };
                let recent_user_messages = db
                    .get_messages_latest(&s.id, 8)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|message| message.role == "user")
                    .rev()
                    .take(2)
                    .map(|message| trim_preview(&message.content.replace('\n', " "), 180))
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!(
                    "- {} [{}] msgs={} status={} summary={} recent_user={}",
                    s.title.clone().unwrap_or_else(|| s.id.clone()),
                    s.source,
                    s.message_count,
                    s.status,
                    summary,
                    recent_user_messages,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let task_lines = task_states
            .iter()
            .filter(|t| recent_for_consolidation(t.updated_at)
                && t.scope_type == "session"
                && recent_sessions.iter().any(|s| s.id == t.scope_id)
                && (t.status == "active" || t.status == "completed"))
            .take(6)
            .map(|t| {
                let spine = t.to_task_spine();
                format!(
                    "- {}:{} status={} goal={} current_step={} pending={} done={}",
                    t.scope_type,
                    t.scope_id,
                    t.status,
                    trim_preview(&spine.goal, 80),
                    trim_preview(&spine.current_step, 80),
                    spine.pending.len(),
                    spine.done.len()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    let task_lines = if task_lines.is_empty() { "- No recent task spines".to_string() } else { task_lines };

    let memory_lines = memories
            .iter()
            .filter(|m| recent_sessions.iter().any(|s| {
                m.evidence_session_id.as_deref() == Some(s.id.as_str())
                    || m.source_session_id.as_deref() == Some(s.id.as_str())
            }))
            .take(8)
            .map(|m| {
                format!(
                    "- [{}] {}",
                    m.category,
                    trim_preview(&m.content.replace('\n', " "), 160)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
    let memory_lines = if memory_lines.is_empty() {
        "- No memories linked to recent sessions".to_string()
    } else { memory_lines };

    format!(
        "## Consolidation Snapshot\n\
         ### Recent Sessions\n{session_lines}\n\n\
         ### Task Spine Snapshot\n{task_lines}\n\n\
         ### Recent Stored Memories\n{memory_lines}\n",
        session_lines = session_lines,
        task_lines = task_lines,
        memory_lines = memory_lines
    )
}

fn resolve_memory_consolidation_prompt(task_prompt: &str, db: &Database) -> String {
    if !is_memory_consolidation_prompt(task_prompt) {
        return task_prompt.to_string();
    }
    format!(
        "{}\n\n{}",
        build_memory_consolidation_snapshot(db),
        task_prompt
    )
}

/// Phase 4c — build a focused consolidation snapshot for a single
/// session. Used by the event-driven consolidation path
/// ([`trigger_consolidation_for_session`]) so we don't blur the
/// snapshot with unrelated sessions.
fn build_session_consolidation_snapshot(db: &Database, session_id: &str) -> String {
    let session = db.get_session(session_id).ok().flatten();
    let task_states = db.list_recent_task_states(100).unwrap_or_default();
    let memories = db.list_memories_for_owner("xiaoheng").unwrap_or_default();

    let session_line = match session.filter(|s| s.source != "scheduled_task" && recent_for_consolidation(s.updated_at)) {
        Some(s) => {
            let summary = if s.rolling_summary.trim().is_empty() {
                "no rolling summary".to_string()
            } else {
                trim_preview(&s.rolling_summary.replace('\n', " "), 320)
            };
            let recent_user = db.get_messages_latest(session_id, 8)
                .unwrap_or_default().into_iter().rev()
                .filter(|message| message.role == "user")
                .take(2)
                .map(|message| trim_preview(&message.content.replace('\n', " "), 180))
                .collect::<Vec<_>>().join(" | ");
            format!(
                "- {} [{}] msgs={} status={} summary={} recent_user={}",
                s.title.clone().unwrap_or_else(|| s.id.clone()),
                s.source,
                s.message_count,
                s.status,
                summary,
                recent_user
            )
        }
        None => format!("- session {} not found", session_id),
    };

    let task_lines = if task_states.is_empty() {
        "- No persisted task spines".to_string()
    } else {
        task_states
            .iter()
            .filter(|t| t.scope_type == "session" && t.scope_id == session_id
                && recent_for_consolidation(t.updated_at))
            .take(4)
            .map(|t| {
                let spine = t.to_task_spine();
                format!(
                    "- {}:{} status={} goal={} current_step={} pending={} done={}",
                    t.scope_type,
                    t.scope_id,
                    t.status,
                    trim_preview(&spine.goal, 80),
                    trim_preview(&spine.current_step, 80),
                    spine.pending.len(),
                    spine.done.len()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let memory_lines = if memories.is_empty() {
        "- No stored memories".to_string()
    } else {
        let relevant: Vec<_> = memories
            .iter()
            .filter(|m| {
                m.evidence_session_id.as_deref() == Some(session_id)
                    || m.source_session_id.as_deref() == Some(session_id)
            })
            .collect();
        let lines = relevant
            .iter()
            .take(6)
            .map(|m| {
                format!(
                    "- [{}/{}] {}",
                    m.category,
                    m.kind,
                    trim_preview(&m.content.replace('\n', " "), 160)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if lines.is_empty() { "- No memories linked to this session".to_string() } else { lines }
    };

    format!(
        "## Session-focused Consolidation Snapshot\n\
         ### Target Session ({})\n{}\n\n\
         ### Related Task Spines\n{}\n\n\
         ### Related Stored Memories\n{}\n",
        session_id, session_line, task_lines, memory_lines
    )
}

/// Phase 4c — event-triggered consolidation for a single session.
///
/// Public API (used in-process from session finalize hooks and when
/// the L2 compaction has run N times for a session). Runs a
/// **lightweight** consolidation bound to the specified session:
/// instead of the wide-snapshot nightly cron, the prompt only sees
/// this session's rolling summary, task spines, and session-anchored
/// memories.
///
/// The existing cron (`0 4 * * *`) is unchanged — this is additive.
///
/// Returns the underlying task id used for the synchronous-spawn run.
pub async fn trigger_consolidation_for_session(
    state: &AppState,
    session_id: &str,
) -> Result<String, String> {
    if session_id.trim().is_empty() {
        return Err("session_id is required".into());
    }
    let task = ensure_memory_consolidation_task_inner(state, None).await?;

    // Build the session-scoped snapshot and append it to the prompt
    // so the agent focuses on this session's residual information.
    let enriched_prompt = {
        let db = state.db.lock().await;
        let snapshot = build_session_consolidation_snapshot(&db, session_id);
        format!(
            "{}\n\n{}\n\n[triggered_by=session_finalize session_id={}]",
            snapshot, task.task_prompt, session_id
        )
    };

    let app_h = state.app_handle.clone();
    let task_id_clone = task.id.clone();
    let db_arc = state.db.clone();
    tokio::spawn(async move {
        execute_task(
            app_h,
            task_id_clone,
            enriched_prompt,
            db_arc,
        )
        .await;
    });

    Ok(task.id)
}

/// Tauri command wrapping [`trigger_consolidation_for_session`] so
/// the frontend (or external integrations) can also fire a
/// session-scoped consolidation on demand.
#[tauri::command]
pub async fn trigger_memory_consolidation_for_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    trigger_consolidation_for_session(&state, &session_id).await
}

pub(crate) async fn register_task_job(state: &AppState, task: &ScheduledTask) {
    unregister_task_job(state, &task.id).await;

    let app_h = state.app_handle.clone();
    let task_id = task.id.clone();
    let task_prompt_clone = task.task_prompt.clone();
    let db_arc = state.db.clone();
    let cron = task.cron_expression.clone();
    let sched = state.scheduler.clone();
    let scheduled_job_ids = state.scheduled_job_ids.clone();
    let task_id_log = task.id.clone();

    match sched
        .add_job(&cron, task_id.clone(), move |_uuid, _sched| {
            let app_h = app_h.clone();
            let task_id = task_id.clone();
            let task_prompt = task_prompt_clone.clone();
            let db_arc = db_arc.clone();
            Box::pin(async move {
                execute_task(
                    app_h,
                    task_id,
                    task_prompt,
                    db_arc,
                )
                .await;
            })
        })
        .await
    {
        Ok(job_id) => {
            let mut jobs = scheduled_job_ids.lock().await;
            jobs.insert(task.id.clone(), job_id);
            info!(
                "Scheduled task {} registered as job {}",
                task_id_log, job_id
            );
        }
        Err(e) => warn!(
            "Failed to register task {} in scheduler: {}",
            task_id_log, e
        ),
    }
}

pub(crate) async fn unregister_task_job(state: &AppState, task_id: &str) {
    let job_id = {
        let mut jobs = state.scheduled_job_ids.lock().await;
        jobs.remove(task_id)
    };
    if let Some(job_id) = job_id {
        if let Err(err) = state.scheduler.remove_job(job_id).await {
            warn!(
                "Failed to remove scheduled job {} for task {}: {}",
                job_id, task_id, err
            );
        } else {
            info!("Removed scheduled job {} for task {}", job_id, task_id);
        }
    }
}

async fn ensure_memory_consolidation_task_inner(
    state: &AppState,
    cron_expression: Option<&str>,
) -> Result<ScheduledTask, String> {
    let cron = cron_expression
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(MEMORY_CONSOLIDATION_DEFAULT_CRON);
    if cron.split_whitespace().count() != 5 {
        return Err(format!(
            "Invalid cron expression '{}': must have 5 parts (minute hour day month weekday)",
            cron
        ));
    }

    let task_prompt = build_memory_consolidation_template();
    let description =
        "Background consolidation of recent sessions, task spines, and durable memories.";

    let task = {
        let db = state.db.lock().await;
        let existing = db
            .list_tasks()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|t| t.name == MEMORY_CONSOLIDATION_TASK_NAME);
        match existing {
            Some(task) => {
                db.update_task(
                    &task.id,
                    Some(MEMORY_CONSOLIDATION_TASK_NAME),
                    Some(cron),
                    Some(&task_prompt),
                    None,
                    Some("active"),
                )
                .map_err(|e| e.to_string())?;
                db.get_task(&task.id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| {
                        "Memory consolidation task disappeared after update".to_string()
                    })?
            }
            None => db
                .create_task(
                    MEMORY_CONSOLIDATION_TASK_NAME,
                    Some(description),
                    cron,
                    &task_prompt,
                    None,
                )
                .map_err(|e| e.to_string())?,
        }
    };

    register_task_job(state, &task).await;
    Ok(task)
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<TaskList, String> {
    let db = state.db.lock().await;
    let tasks = db.list_tasks().map_err(|e| e.to_string())?;
    let total = tasks.len();
    Ok(TaskList { tasks, total })
}

#[tauri::command]
pub async fn create_task(
    state: State<'_, AppState>,
    name: String,
    description: Option<String>,
    cron_expression: String,
    task_prompt: String,
    notify_targets: Option<Vec<String>>,
) -> Result<ScheduledTask, String> {
    let parts: Vec<&str> = cron_expression.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!(
            "Invalid cron expression '{}': must have 5 parts (minute hour day month weekday)",
            cron_expression
        ));
    }

    let notify_targets_json = match notify_targets {
        Some(tokens) if !tokens.is_empty() => {
            Some(serde_json::to_string(&tokens).map_err(|e| e.to_string())?)
        }
        _ => None,
    };

    let task = {
        let db = state.db.lock().await;
        db.create_task(
            &name,
            description.as_deref(),
            &cron_expression,
            &task_prompt,
            notify_targets_json.as_deref(),
        )
        .map_err(|e| e.to_string())?
    };
    register_task_job(&state, &task).await;

    Ok(task)
}

#[tauri::command]
pub async fn ensure_memory_consolidation_task(
    state: State<'_, AppState>,
    cron_expression: Option<String>,
) -> Result<ScheduledTask, String> {
    ensure_memory_consolidation_task_inner(&state, cron_expression.as_deref()).await
}

#[tauri::command]
pub async fn run_memory_consolidation_now(
    state: State<'_, AppState>,
    cron_expression: Option<String>,
) -> Result<String, String> {
    let task = ensure_memory_consolidation_task_inner(&state, cron_expression.as_deref()).await?;
    run_task_now(state, task.id).await
}

#[tauri::command]
pub async fn update_task(
    state: State<'_, AppState>,
    task_id: String,
    name: Option<String>,
    cron_expression: Option<String>,
    task_prompt: Option<String>,
    notify_targets: Option<Vec<String>>,
    status: Option<String>,
) -> Result<(), String> {
    let notify_targets_json = match notify_targets {
        Some(tokens) => Some(serde_json::to_string(&tokens).map_err(|e| e.to_string())?),
        None => None,
    };
    {
        let db = state.db.lock().await;
        db.update_task(
            &task_id,
            name.as_deref(),
            cron_expression.as_deref(),
            task_prompt.as_deref(),
            notify_targets_json.as_deref(),
            status.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    }

    let task = {
        let db = state.db.lock().await;
        db.get_task(&task_id).map_err(|e| e.to_string())?
    };

    match task {
        Some(task) if task.status == "active" => register_task_job(&state, &task).await,
        Some(_) | None => unregister_task_job(&state, &task_id).await,
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_task(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    unregister_task_job(&state, &task_id).await;
    let db = state.db.lock().await;
    db.delete_task(&task_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_task_now(state: State<'_, AppState>, task_id: String) -> Result<String, String> {
    let task = {
        let db = state.db.lock().await;
        db.get_task(&task_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Task {} not found", task_id))?
    };

    // Execute the task asynchronously
    let app_h = state.app_handle.clone();
    let task_id_clone = task.id.clone();
    let task_prompt = task.task_prompt.clone();
    let db_arc = state.db.clone();

    tokio::spawn(async move {
        execute_task(
            app_h,
            task_id_clone,
            task_prompt,
            db_arc,
        )
        .await;
    });

    Ok(format!("Task '{}' triggered manually", task.name))
}

/// Trigger a task from external events (webhook/email relay).
/// The payload is appended to the task prompt as contextual input.
#[tauri::command]
pub async fn trigger_task_by_event(
    state: State<'_, AppState>,
    task_id: String,
    trigger_type: String,
    payload: Option<String>,
) -> Result<String, String> {
    let task = {
        let db = state.db.lock().await;
        db.get_task(&task_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Task {} not found", task_id))?
    };

    let enriched_prompt = if let Some(p) = payload {
        format!(
            "{}\n\n[trigger_type={}]\n[payload]\n{}",
            task.task_prompt, trigger_type, p
        )
    } else {
        format!("{}\n\n[trigger_type={}]", task.task_prompt, trigger_type)
    };

    let app_h = state.app_handle.clone();
    let task_id_clone = task.id.clone();
    let db_arc = state.db.clone();
    tokio::spawn(async move {
        execute_task(
            app_h,
            task_id_clone,
            enriched_prompt,
            db_arc,
        )
        .await;
    });

    Ok(format!(
        "Task '{}' triggered by {}",
        task.name, trigger_type
    ))
}

/// Core task execution through Codex exec.
/// Emits progress on the "scheduler_task_{task_id}" Tauri event channel.
pub async fn execute_task(
    app: AppHandle,
    task_id: String,
    task_prompt: String,
    db: Arc<Mutex<Database>>,
) {
    info!("Executing scheduled task with codex exec: {}", task_id);
    let task_name = {
        let db = db.lock().await;
        let _ = db.record_task_run(&task_id);
        db.get_task(&task_id).ok().flatten().map(|task| task.name)
    };
    let session_title = build_scheduler_session_title(task_name.as_deref(), &task_id);
    let effective_task_prompt = {
        let db = db.lock().await;
        resolve_memory_consolidation_prompt(&task_prompt, &db)
    };
    let notify_context = {
        let db = db.lock().await;
        db.get_scheduled_task_notify_targets(&task_id)
            .ok()
            .flatten()
            .filter(|targets| !targets.trim().is_empty())
            .map(|targets| {
                format!(
                    "本任务配置的通知目标为：{targets}。需要发送结果时，使用 WorkBuddy MCP 的 IM 工具解析并投递，不要猜测收件人。"
                )
            })
            .unwrap_or_default()
    };
    let scope_id = format!("sched_{task_id}");
    let previous_memory_revision = if is_memory_consolidation_prompt(&task_prompt) {
        let db = db.lock().await;
        Some(memory_revision(&db))
    } else {
        None
    };
    {
        let db = db.lock().await;
        let _ = db.update_task_run_status(&task_id, "running");
    }
    let _ = app.emit(
        &format!("task_status_{task_id}"),
        serde_json::json!({ "status": "running" }),
    );
    let state = app.state::<AppState>();
    let result = run_codex_background(
        &state,
        &scope_id,
        &effective_task_prompt,
        None,
        "scheduler",
        CodexBackgroundOptions {
            session_title: Some(session_title),
            session_source: Some("scheduled_task".to_string()),
            extra_context: Some(format!(
                "这是 WorkBuddy 定时任务。任务 ID：{task_id}。{notify_context}"
            )),
            event_channel: Some(format!("scheduler_task_{task_id}")),
            ..Default::default()
        },
    )
    .await;
    let final_status = if result.is_err() {
        "failed"
    } else if let Some(previous) = previous_memory_revision {
        let db = db.lock().await;
        if memory_revision(&db) == previous {
            "no_change"
        } else {
            "success"
        }
    } else {
        "success"
    };
    {
        let db = db.lock().await;
        let _ = db.update_task_run_status(&task_id, final_status);
    }
    let _ = app.emit(
        &format!("task_status_{task_id}"),
        serde_json::json!({
            "status": final_status,
            "error": result.as_ref().err(),
        }),
    );
    info!(
        "Scheduled task {} completed through codex exec (success={})",
        task_id,
        result.is_ok()
    );
}

#[cfg(test)]
mod memory_consolidation_tests {
    use super::{build_memory_consolidation_snapshot, build_memory_consolidation_template};
    use crate::store::Database;

    #[test]
    fn consolidation_uses_the_installed_workbuddy_mcp_memory_tools() {
        let prompt = build_memory_consolidation_template();
        assert!(prompt.contains("memory_search"));
        assert!(prompt.contains("memory_append"));
        assert!(!prompt.contains("memory_store"));
    }

    #[test]
    fn consolidation_snapshot_includes_recent_user_content_without_a_rolling_summary() {
        let db = Database::open_in_memory().expect("database");
        let session = db.create_session(Some("腕表内容偏好")).expect("session");
        db.append_message(&session.id, "user", "我以后都希望报告使用简洁的中文结论")
            .expect("message");

        let snapshot = build_memory_consolidation_snapshot(&db);
        assert!(snapshot.contains("简洁的中文结论"), "{snapshot}");
    }

    #[test]
    fn consolidation_ignores_stale_sessions_and_task_spines() {
        let root = std::env::temp_dir().join(format!("workbuddy-snapshot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("workbuddy.db");
        let db = Database::open(&path).unwrap();
        let stale = db.create_session(Some("旧应用小红书任务")).unwrap();
        db.append_message(&stale.id, "user", "发布旧版小红书内容").unwrap();
        let spine = db.get_or_create_task_state("session", &stale.id).unwrap();
        db.update_task_state(&spine.id, Some("处理旧版小红书任务"), None, None, Some("active")).unwrap();
        let recent = db.create_session(Some("当前任务")).unwrap();
        db.append_message(&recent.id, "user", "这次报告采用简洁中文").unwrap();
        db.save_memory("旧品牌推广偏好", "preference", 0.8, Some(&stale.id), "xiaoheng", "private", "xiaoheng", None).unwrap();
        for _ in 0..25 {
            db.create_session_with_source(Some("每日整理"), "scheduled_task").unwrap();
        }

        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute("UPDATE sessions SET updated_at='2026-06-22T00:00:00Z' WHERE id=?1", [&stale.id]).unwrap();
        connection.execute("UPDATE task_states SET updated_at='2026-06-22T00:00:00Z' WHERE id=?1", [&spine.id]).unwrap();

        let snapshot = build_memory_consolidation_snapshot(&db);
        assert!(!snapshot.contains("旧应用小红书任务"), "{snapshot}");
        assert!(!snapshot.contains("处理旧版小红书任务"), "{snapshot}");
        assert!(!snapshot.contains("旧品牌推广偏好"), "{snapshot}");
        assert!(snapshot.contains("这次报告采用简洁中文"), "{snapshot}");
        let _ = std::fs::remove_dir_all(root);
    }
}
