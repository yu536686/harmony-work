use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub async fn respond_permission(
    state: State<'_, AppState>,
    codex_state: State<'_, crate::codex_runtime::service::CodexRuntimeState>,
    request_id: String,
    approved: bool,
    for_session: Option<bool>,
) -> Result<(), String> {
    let mut map = state.confirmation_responses.lock().await;
    if let Some(tx) = map.remove(&request_id) {
        let _ = tx.send(approved);
        Ok(())
    } else {
        drop(map);
        codex_state
            .respond_approval(&request_id, approved, for_session.unwrap_or(false))
            .await
            .map_err(|error| error.to_string())
    }
}
