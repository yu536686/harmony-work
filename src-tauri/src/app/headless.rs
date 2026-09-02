//! Headless CLI execution path.
//!
//! Extracted from the old monolithic `desktop_app.rs`. Entrypoint is
//! [`run_cli_headless_request`] which binds an already-built `AppState`
//! to one of the two CLI modes (`Codex` single-agent or `Pool`
//! coordinator), wires up tool overrides, and optionally blocks until
//! the pool's todos converge. Result serialization + error-file writing
//! is handled by [`persist_headless_cli_result`] so the bootstrap layer
//! stays out of the JSON plumbing.

use crate::{
    codex_runtime::background::{run_codex_background, CodexBackgroundOptions},
    commands,
    headless_cli::{
        disabled_tools_for_mode, DisabledToolInfo, HeadlessCliMode, HeadlessCliRequest,
        HeadlessCliResponse, PoolWaitSummary,
    },
    store,
};
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;

pub type CliResultSink = Arc<StdMutex<Option<Result<HeadlessCliResponse, String>>>>;

pub fn persist_headless_cli_result(
    output: Option<&str>,
    result: &Result<HeadlessCliResponse, String>,
) -> Result<(), String> {
    match result {
        Ok(response) => {
            let json = serde_json::to_string_pretty(response)
                .map_err(|e| format!("Serialize failed: {}", e))?;
            if let Some(path) = output.map(str::trim).filter(|s| !s.is_empty()) {
                let path = std::path::Path::new(path);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed to create '{}': {}", parent.display(), e)
                        })?;
                    }
                }
                std::fs::write(path, format!("{}\n", json))
                    .map_err(|e| format!("Failed to write '{}': {}", path.display(), e))?;
            } else {
                println!("{}", json);
            }
            Ok(())
        }
        Err(error) => {
            if let Some(path) = output.map(str::trim).filter(|s| !s.is_empty()) {
                let path = std::path::Path::new(path);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("Failed to create '{}': {}", parent.display(), e)
                        })?;
                    }
                }
                let payload = serde_json::json!({
                    "ok": false,
                    "error": error,
                });
                std::fs::write(path, format!("{}\n", payload))
                    .map_err(|e| format!("Failed to write '{}': {}", path.display(), e))?;
            }
            if !error.is_empty() {
                eprintln!("{}", error);
            }
            Err(error.clone())
        }
    }
}

fn cli_disabled_tools(mode: HeadlessCliMode) -> Vec<DisabledToolInfo> {
    disabled_tools_for_mode(mode)
}

fn cli_extra_system_context(request: &HeadlessCliRequest) -> String {
    let mut lines = vec![
        "## Headless CLI Runtime".to_string(),
        format!("- Mode: {}", request.mode.as_str()),
        format!("- Host OS: {}", std::env::consts::OS),
        "- This is a non-interactive headless CLI session.".to_string(),
    ];
    let disabled = cli_disabled_tools(request.mode);
    if !disabled.is_empty() {
        lines.push("- Disabled tools in this runtime:".to_string());
        for tool in &disabled {
            lines.push(format!("  - {}: {}", tool.name, tool.reason));
        }
    }
    match request.mode {
        HeadlessCliMode::Codex => {
            lines.push(
                "- Stay single-agent. Do not create or manage collaborative pool work in this run."
                    .to_string(),
            );
        }
        HeadlessCliMode::Pool => {
            lines.push(
                "- You are coordinating a StaffDeck project. Use staff_project_list, staff_task_list, staff_list, staff_task_create, staff_task_update and staff_status_set."
                    .to_string(),
            );
            lines.push(
                "- Create only concrete, non-duplicate tasks. A staff_task_create call is executed by the Codex StaffDeck dispatcher; review its reported artifacts before marking work done."
                    .to_string(),
            );
            lines.push(
                "- If this pool run needs to notify a human through IM, use the explicit routing sequence: im_channel_list -> im_channel_connect (if needed) -> im_channel_binding_lookup(pool_id=...) -> im_send_message. Do not guess binding_key values or claim delivery when no binding exists."
                    .to_string(),
            );
            if let Some(size) = request.pool_size {
                lines.push(format!(
                    "- Target collaboration scale: at most {} Koi unless the task clearly needs fewer.",
                    size
                ));
            }
            if !request.koi_ids.is_empty() {
                lines.push(format!(
                    "- Prefer coordinating these Koi IDs first: {}.",
                    request.koi_ids.join(", ")
                ));
            }
        }
    }
    if let Some(extra) = request
        .extra_system_context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lines.push(String::new());
        lines.push("## Additional Context".to_string());
        lines.push(extra.to_string());
    }
    lines.join("\n")
}

async fn resolve_cli_pool(
    state: &store::AppState,
    request: &HeadlessCliRequest,
) -> Result<crate::pool::PoolSession, String> {
    let db = state.db.lock().await;
    if let Some(requested) = request
        .pool_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let pool = db
            .resolve_pool_session_identifier(requested)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Pool '{}' not found.", requested))?;
        if request.task_timeout_secs.is_some() {
            db.update_pool_session_config(&pool.id, request.task_timeout_secs)
                .map_err(|e| e.to_string())?;
        }
        return Ok(pool);
    }

    let name = request
        .pool_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Headless Pool Run");
    db.create_pool_session_with_dir(
        name,
        request.workspace.as_deref(),
        request.task_timeout_secs.unwrap_or(0),
    )
    .map_err(|e| e.to_string())
}

async fn wait_for_pool_completion(
    state: &store::AppState,
    pool_id: &str,
    timeout_secs: u64,
) -> Result<PoolWaitSummary, String> {
    let start = std::time::Instant::now();
    let idle_grace = std::time::Duration::from_secs(3);
    let timeout = std::time::Duration::from_secs(timeout_secs.max(1));
    let mut zero_since: Option<std::time::Instant> = None;

    loop {
        let (active, done, cancelled, blocked, latest_messages) = {
            let db = state.db.lock().await;
            let todos = db.list_koi_todos(None).map_err(|e| e.to_string())?;
            let pool_todos = todos
                .into_iter()
                .filter(|todo| todo.pool_session_id.as_deref() == Some(pool_id))
                .collect::<Vec<_>>();
            let active = pool_todos
                .iter()
                .filter(|todo| matches!(todo.status.as_str(), "todo" | "in_progress" | "blocked"))
                .count() as u32;
            let done = pool_todos
                .iter()
                .filter(|todo| todo.status == "done")
                .count() as u32;
            let cancelled = pool_todos
                .iter()
                .filter(|todo| todo.status == "cancelled")
                .count() as u32;
            let blocked = pool_todos
                .iter()
                .filter(|todo| todo.status == "blocked")
                .count() as u32;
            let latest_messages = db
                .get_pool_messages(pool_id, 10, 0)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|msg| {
                    format!(
                        "#{} {} ({}): {}",
                        msg.id,
                        msg.sender_id,
                        msg.msg_type,
                        msg.content.chars().take(240).collect::<String>()
                    )
                })
                .collect::<Vec<_>>();
            (active, done, cancelled, blocked, latest_messages)
        };

        if active == 0 {
            match zero_since {
                Some(since) if since.elapsed() >= idle_grace => {
                    let requires_supervisor_closeout = done > 0;
                    let closeout_status = if requires_supervisor_closeout {
                        "awaiting_supervisor_closeout"
                    } else {
                        "idle_no_work"
                    };
                    return Ok(PoolWaitSummary {
                        completed: true,
                        timed_out: false,
                        closeout_status: closeout_status.to_string(),
                        requires_supervisor_closeout,
                        active_todos: active,
                        done_todos: done,
                        cancelled_todos: cancelled,
                        blocked_todos: blocked,
                        latest_messages,
                    });
                }
                None => zero_since = Some(std::time::Instant::now()),
                _ => {}
            }
        } else {
            zero_since = None;
        }

        if start.elapsed() >= timeout {
            return Ok(PoolWaitSummary {
                completed: false,
                timed_out: true,
                closeout_status: "timed_out".to_string(),
                requires_supervisor_closeout: false,
                active_todos: active,
                done_todos: done,
                cancelled_todos: cancelled,
                blocked_todos: blocked,
                latest_messages,
            });
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

pub async fn run_cli_headless_request(
    state: store::AppState,
    app_handle: tauri::AppHandle,
    request: HeadlessCliRequest,
) -> Result<HeadlessCliResponse, String> {
    let workspace_override = request
        .workspace
        .as_deref()
        .map(str::trim)
        .filter(|workspace| !workspace.is_empty())
        .map(std::path::PathBuf::from);

    let extra_context = cli_extra_system_context(&request);
    let disabled_tools = cli_disabled_tools(request.mode);

    let (session_id, pool_id) = match request.mode {
        HeadlessCliMode::Codex => (
            request
                .session_id
                .clone()
                .unwrap_or_else(|| format!("headless_cli_{}", chrono::Utc::now().timestamp())),
            None,
        ),
        HeadlessCliMode::Pool => {
            let pool = resolve_cli_pool(&state, &request).await?;
            (
                request
                    .session_id
                    .clone()
                    .unwrap_or_else(|| commands::chat::pool_coordinator_session_id(&pool.id)),
                Some(pool.id),
            )
        }
    };

    let session_title = request
        .session_title
        .clone()
        .unwrap_or_else(|| match request.mode {
            HeadlessCliMode::Codex => "Headless CLI Task".to_string(),
            HeadlessCliMode::Pool => "Headless Pool Coordinator".to_string(),
        });
    let session_source = match request.mode {
        HeadlessCliMode::Codex => "codex_cli".to_string(),
        HeadlessCliMode::Pool => "codex_staffdeck".to_string(),
    };
    let channel = request.channel.clone().unwrap_or_else(|| "cli".to_string());

    let output = run_codex_background(
        &state,
        &session_id,
        &request.prompt,
        None,
        &channel,
        CodexBackgroundOptions {
            session_title: Some(session_title),
            session_source: Some(session_source),
            workspace_root: workspace_override,
            extra_context: Some(extra_context),
            ..Default::default()
        },
    )
    .await?;

    let pool_wait = if request.mode == HeadlessCliMode::Pool && request.wait_for_completion {
        Some(
            wait_for_pool_completion(
                &state,
                pool_id.as_deref().unwrap_or_default(),
                request.wait_timeout_secs.unwrap_or(900),
            )
            .await?,
        )
    } else {
        None
    };

    let _ = app_handle.emit(
        "headless_cli_completed",
        json!({ "session_id": session_id, "mode": request.mode.as_str() }),
    );

    Ok(HeadlessCliResponse {
        ok: true,
        mode: request.mode.as_str().to_string(),
        session_id,
        pool_id,
        response_text: output.final_message,
        disabled_tools,
        pool_wait,
    })
}
