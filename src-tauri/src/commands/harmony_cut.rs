use crate::harmony_cut::{
    continue_project, get_project, health, list_projects, prepare_project, start_service,
    HarmonyCutContinueInput, HarmonyCutContinueOutput, HarmonyCutHealth, HarmonyCutPrepareInput,
    HarmonyCutPrepareOutput, HarmonyCutProjectSummary,
};
use serde_json::Value;

#[tauri::command]
pub async fn harmony_cut_health() -> Result<HarmonyCutHealth, String> {
    Ok(health().await)
}

#[tauri::command]
pub async fn harmony_cut_start_service() -> Result<HarmonyCutHealth, String> {
    start_service().await
}

#[tauri::command]
pub async fn harmony_cut_prepare(
    request: HarmonyCutPrepareInput,
) -> Result<HarmonyCutPrepareOutput, String> {
    prepare_project(request).await
}

#[tauri::command]
pub async fn harmony_cut_continue(
    request: HarmonyCutContinueInput,
) -> Result<HarmonyCutContinueOutput, String> {
    continue_project(request).await
}

#[tauri::command]
pub async fn harmony_cut_projects() -> Result<Vec<HarmonyCutProjectSummary>, String> {
    list_projects().await
}

#[tauri::command]
pub async fn harmony_cut_project(project_id: String) -> Result<Value, String> {
    get_project(&project_id).await
}
