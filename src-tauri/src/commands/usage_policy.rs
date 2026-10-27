use crate::store::AppState;
use crate::store::db::{UsageBudget, UsageOverview};
use tauri::State;

pub(crate) async fn check_budget(state: &AppState, session_id: &str) -> Result<(), String> {
    let root = state.settings.lock().await.workspace_root.clone();
    state.db.lock().await.usage_overview(session_id, &root)
        .and_then(|usage| usage.check()).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn usage_get(state: State<'_, AppState>, session_id: String) -> Result<UsageOverview, String> {
    let root = state.settings.lock().await.workspace_root.clone();
    state.db.lock().await.usage_overview(&session_id, &root).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn usage_set_budget(state: State<'_, AppState>, session_id: String, budget: UsageBudget) -> Result<UsageOverview, String> {
    let root = state.settings.lock().await.workspace_root.clone();
    state.db.lock().await.set_usage_budget(&session_id, &root, budget).map_err(|error| error.to_string())
}
