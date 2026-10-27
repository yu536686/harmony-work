//! Workspace/session-scoped file review and explicitly confirmed restoration.
//! Host capture hooks are synchronous; invoke them on blocking workers.
//! Old snapshot list commands remain available, but unsafe legacy undo is refused.

use std::path::Path;

use crate::file_journal::{FileJournal, JournalChange, JournalDiff, JournalReview};
use tauri::State;

use crate::commands::chat::resolve_session_workspace_root;
use crate::store::AppState;

fn open_workspace_journal(workspace_root: &str) -> Result<FileJournal, String> {
    let root = std::fs::canonicalize(Path::new(workspace_root)).map_err(|e| e.to_string())?;
    let db = root.join(".xiaoheng").join("journal.db");
    FileJournal::open(root, db).map_err(|e| e.to_string())
}

/// Host-only entry point. Run on a blocking worker before runtime start, and
/// preserve this exact workspace/session/turn tuple until terminal completion.
#[allow(dead_code)] // Registered/called by the parent integration, not this patch.
pub fn capture_turn_before(
    workspace_root: &str,
    session_id: &str,
    turn_key: &str,
) -> Result<(), String> {
    open_workspace_journal(workspace_root)?
        .capture_before(session_id, turn_key)
        .map_err(|e| e.to_string())
}

/// Host-only entry point, including failed/cancelled terminal outcomes. Call
/// only after workspace writers stop. A failure is observable and retryable.
#[allow(dead_code)]
pub fn capture_turn_after(
    workspace_root: &str,
    session_id: &str,
    turn_key: &str,
) -> Result<(), String> {
    open_workspace_journal(workspace_root)?
        .capture_after(session_id, turn_key)
        .map_err(|e| e.to_string())
}

/// Recovery only: the host must first establish that the crashed/failed turn has
/// no remaining writers. Never substitutes a later workspace state as "after".
#[allow(dead_code)]
pub fn abandon_turn_capture(
    workspace_root: &str,
    session_id: &str,
    turn_key: &str,
) -> Result<(), String> {
    open_workspace_journal(workspace_root)?
        .abandon_capture(session_id, turn_key)
        .map_err(|e| e.to_string())
}

/// Nonterminal invalidation (for example, an uncertain local cancel/stream
/// disconnect). Keeps active guards until a trusted final after/abandon call.
#[allow(dead_code)]
pub fn mark_turn_capture_unavailable(
    workspace_root: &str,
    session_id: &str,
    turn_key: &str,
    reason: &str,
) -> Result<(), String> {
    open_workspace_journal(workspace_root)?
        .mark_capture_unavailable(session_id, turn_key, reason)
        .map_err(|e| e.to_string())
}

async fn checked_session_workspace(
    state: &State<'_, AppState>,
    session_id: &str,
    supplied: &str,
) -> Result<String, String> {
    let effective = session_workspace(state, session_id).await?;
    let effective =
        std::fs::canonicalize(effective).map_err(|e| format!("WORKSPACE_UNAVAILABLE: {e}"))?;
    let requested =
        std::fs::canonicalize(supplied).map_err(|e| format!("WORKSPACE_UNAVAILABLE: {e}"))?;
    if effective != requested {
        return Err("WORKSPACE_MISMATCH: session workspace changed; reopen file review in the current workspace".into());
    }
    effective
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "UNSUPPORTED_PATH: non-UTF8 workspace".into())
}

#[allow(dead_code)] // Add these three commands to generate_handler! in parent.
#[tauri::command]
pub async fn journal_review(
    state: State<'_, AppState>,
    session_id: String,
    workspace_root: String,
    turn_key: Option<String>,
) -> Result<JournalReview, String> {
    let root = checked_session_workspace(&state, &session_id, &workspace_root).await?;
    tauri::async_runtime::spawn_blocking(move || {
        open_workspace_journal(&root)?
            .review(&session_id, turn_key.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[allow(dead_code)]
#[tauri::command]
pub async fn journal_diff(
    state: State<'_, AppState>,
    session_id: String,
    workspace_root: String,
    turn_key: String,
    path: String,
) -> Result<JournalDiff, String> {
    let root = checked_session_workspace(&state, &session_id, &workspace_root).await?;
    tauri::async_runtime::spawn_blocking(move || {
        open_workspace_journal(&root)?
            .diff(&session_id, &turn_key, &path)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[allow(dead_code)]
#[tauri::command]
pub async fn journal_restore_file(
    state: State<'_, AppState>,
    session_id: String,
    workspace_root: String,
    turn_key: String,
    path: String,
    expected_after_hash: String,
    confirmed: bool,
) -> Result<(), String> {
    let root = checked_session_workspace(&state, &session_id, &workspace_root).await?;
    tauri::async_runtime::spawn_blocking(move || {
        open_workspace_journal(&root)?
            .restore_file(
                &session_id,
                &turn_key,
                &path,
                &expected_after_hash,
                confirmed,
            )
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Resolve the session's effective workspace root (per-session override, else
/// the global setting) so the frontend only needs to pass a session id.
async fn session_workspace(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<String, String> {
    let default_root = { state.settings.lock().await.workspace_root.clone() };
    resolve_session_workspace_root(state, session_id, default_root).await
}

/// Files changed by the most recent turn (applied, not yet undone), newest first.
#[tauri::command]
pub async fn journal_list_changes(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<JournalChange>, String> {
    let root = session_workspace(&state, &session_id).await?;
    let journal = open_workspace_journal(&root)?;
    match journal
        .latest_turn_with_changes(&session_id)
        .map_err(|e| e.to_string())?
    {
        Some(turn) => journal
            .list_changes(&session_id, &turn)
            .map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

/// Compatibility endpoint: legacy snapshots cannot prove current ownership, so
/// an existing legacy change returns a confirmation/safety error without writes.
#[tauri::command]
pub async fn journal_undo_last(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<String>, String> {
    let root = session_workspace(&state, &session_id).await?;
    let journal = open_workspace_journal(&root)?;
    match journal
        .latest_turn_with_changes(&session_id)
        .map_err(|e| e.to_string())?
    {
        Some(turn) => journal
            .undo_turn(&session_id, &turn)
            .map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}
