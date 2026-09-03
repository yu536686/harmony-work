use crate::harmony_cut::{
    continue_project, get_project, health, list_projects, prepare_project, start_service,
    HarmonyCutContinueInput, HarmonyCutContinueOutput, HarmonyCutHealth, HarmonyCutPrepareInput,
    HarmonyCutPrepareOutput, HarmonyCutProjectSummary,
};
use serde_json::Value;
use tauri::AppHandle;

#[tauri::command]
pub async fn harmony_cut_health(app: AppHandle) -> Result<HarmonyCutHealth, String> {
    Ok(health(Some(&app)).await)
}

#[tauri::command]
pub async fn harmony_cut_start_service(app: AppHandle) -> Result<HarmonyCutHealth, String> {
    start_service(Some(&app)).await
}

#[tauri::command]
pub async fn harmony_cut_prepare(
    request: HarmonyCutPrepareInput,
    app: AppHandle,
) -> Result<HarmonyCutPrepareOutput, String> {
    prepare_project(request, Some(&app)).await
}

#[tauri::command]
pub async fn harmony_cut_continue(
    request: HarmonyCutContinueInput,
    app: AppHandle,
) -> Result<HarmonyCutContinueOutput, String> {
    continue_project(request, Some(&app)).await
}

#[tauri::command]
pub async fn harmony_cut_projects(app: AppHandle) -> Result<Vec<HarmonyCutProjectSummary>, String> {
    list_projects(Some(&app)).await
}

#[tauri::command]
pub async fn harmony_cut_project(project_id: String, app: AppHandle) -> Result<Value, String> {
    get_project(&project_id, Some(&app)).await
}
