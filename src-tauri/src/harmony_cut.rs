use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Manager;

pub const DEFAULT_HARMONY_CUT_ROOT: &str = r"D:\harmony_cut";
pub const DEFAULT_HARMONY_CUT_URL: &str = "http://127.0.0.1:8765";
const BUNDLED_HARMONY_CUT_DIR: &str = "harmony-cut";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutHealth {
    pub installed: bool,
    pub server_running: bool,
    pub root: String,
    pub server_url: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutPrepareInput {
    pub media_dir: String,
    pub title: Option<String>,
    pub requirement: Option<String>,
    pub auto_rotate_landscape: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutPrepareOutput {
    pub project_id: String,
    pub review_url: String,
    pub workbuddy_url: String,
    pub marker_files: Value,
    pub next_instruction: String,
    pub project: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutContinueInput {
    pub project_id: Option<String>,
    pub skip_preview: Option<bool>,
    pub skip_draft: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutContinueOutput {
    pub project_id: String,
    pub preview: Value,
    pub draft: Value,
    pub marker_files: Value,
    pub project: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarmonyCutProjectSummary {
    pub id: String,
    pub title: String,
    pub media_dir: String,
    pub workflow_status: String,
    pub review_url: String,
}

fn env_root_dir() -> Option<PathBuf> {
    std::env::var("WORKBUDDY_HARMONY_CUT_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn bundled_source_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;
    [
        resource_dir.join(BUNDLED_HARMONY_CUT_DIR),
        resource_dir.join("resources").join(BUNDLED_HARMONY_CUT_DIR),
    ]
    .into_iter()
    .find(|path| path.join("server.py").exists())
}

fn copy_runtime_entry(source: &Path, target: &Path) -> Result<(), String> {
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if name == "data" || name == "__pycache__" {
        return Ok(());
    }

    let metadata = fs::metadata(source)
        .map_err(|e| format!("Failed to read Harmony Cut resource metadata: {e}"))?;
    if metadata.is_dir() {
        fs::create_dir_all(target)
            .map_err(|e| format!("Failed to create Harmony Cut runtime dir: {e}"))?;
        for entry in fs::read_dir(source)
            .map_err(|e| format!("Failed to list Harmony Cut resource dir: {e}"))?
        {
            let entry = entry.map_err(|e| format!("Failed to read Harmony Cut resource: {e}"))?;
            copy_runtime_entry(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create Harmony Cut runtime parent dir: {e}"))?;
        }
        fs::copy(source, target).map_err(|e| {
            format!(
                "Failed to copy Harmony Cut resource {} to {}: {e}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn sync_bundled_runtime(app: &tauri::AppHandle, source: &Path) -> Result<PathBuf, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    let target = app_data_dir.join(BUNDLED_HARMONY_CUT_DIR);
    fs::create_dir_all(&target)
        .map_err(|e| format!("Failed to create Harmony Cut runtime dir: {e}"))?;
    for entry in fs::read_dir(source)
        .map_err(|e| format!("Failed to list bundled Harmony Cut resources: {e}"))?
    {
        let entry =
            entry.map_err(|e| format!("Failed to read bundled Harmony Cut resource: {e}"))?;
        copy_runtime_entry(&entry.path(), &target.join(entry.file_name()))?;
    }
    Ok(target)
}

pub fn root_dir(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
    if let Some(root) = env_root_dir() {
        return Ok(root);
    }
    if let Some(app) = app {
        if let Some(source) = bundled_source_dir(app) {
            return sync_bundled_runtime(app, &source);
        }
    }
    Ok(PathBuf::from(DEFAULT_HARMONY_CUT_ROOT))
}

pub fn server_url() -> String {
    std::env::var("WORKBUDDY_HARMONY_CUT_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_HARMONY_CUT_URL.to_string())
}

pub fn workbuddy_project_url(project_id: &str) -> String {
    format!(
        "/harmony-cut?project_id={}",
        urlencoding::encode(project_id)
    )
}

fn client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("Xiaoheng-WorkBuddy/HarmonyCut")
        .build()
        .map_err(|e| e.to_string())
}

fn output_text(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn check_python_dependencies() -> Result<(), String> {
    let output = piscis_kernel::proc::std_command("python")
        .arg("-c")
        .arg("import starlette, uvicorn, pydantic, cv2, numpy, imageio_ffmpeg")
        .output()
        .map_err(|e| format!("Failed to run python dependency check: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(output_text(&output))
    }
}

async fn ensure_python_dependencies(root: PathBuf) -> Result<(), String> {
    if check_python_dependencies().is_ok() {
        return Ok(());
    }
    let requirements = root.join("requirements.txt");
    if !requirements.exists() {
        return Err(format!(
            "Harmony Cut Python dependencies are missing and requirements.txt was not found: {}",
            requirements.display()
        ));
    }
    tokio::task::spawn_blocking(move || {
        let output = piscis_kernel::proc::std_command("python")
            .arg("-m")
            .arg("pip")
            .arg("install")
            .arg("--user")
            .arg("-r")
            .arg(&requirements)
            .output()
            .map_err(|e| format!("Failed to install Harmony Cut Python dependencies: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "Failed to install Harmony Cut Python dependencies:\n{}",
                output_text(&output)
            ));
        }
        check_python_dependencies().map_err(|e| {
            format!("Harmony Cut Python dependencies are still unavailable after install:\n{e}")
        })
    })
    .await
    .map_err(|e| format!("Harmony Cut dependency task failed: {e}"))?
}

async fn json_or_error(resp: reqwest::Response, label: &str) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{label} returned HTTP {status}: {text}"));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{label} response is not valid JSON: {e}; body={text}"))?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        let message = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Harmony Cut request failed");
        return Err(message.to_string());
    }
    Ok(value)
}

pub async fn server_running() -> bool {
    let Ok(client) = client(2) else {
        return false;
    };
    match client
        .get(format!("{}/api/projects", server_url()))
        .send()
        .await
    {
        Ok(resp) => resp.status() == StatusCode::OK,
        Err(_) => false,
    }
}

pub async fn health(app: Option<&tauri::AppHandle>) -> HarmonyCutHealth {
    let root_result = root_dir(app);
    let root = root_result
        .as_ref()
        .cloned()
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_HARMONY_CUT_ROOT));
    let installed = root_result.is_ok() && root.join("server.py").exists();
    let server_running = if installed {
        server_running().await
    } else {
        false
    };
    let message = if let Err(error) = root_result {
        error
    } else if !installed {
        format!("Harmony Cut root not found: {}", root.display())
    } else if server_running {
        "Harmony Cut service is running".to_string()
    } else {
        "Harmony Cut is installed but the local service is not running".to_string()
    };
    HarmonyCutHealth {
        installed,
        server_running,
        root: root.to_string_lossy().to_string(),
        server_url: server_url(),
        message,
    }
}

pub async fn start_service(app: Option<&tauri::AppHandle>) -> Result<HarmonyCutHealth, String> {
    let root = root_dir(app)?;
    let server_py = root.join("server.py");
    if !server_py.exists() {
        return Err(format!(
            "Harmony Cut server.py not found: {}",
            server_py.display()
        ));
    }
    if server_running().await {
        return Ok(health(app).await);
    }
    ensure_python_dependencies(root.clone()).await?;

    let mut cmd = piscis_kernel::proc::std_command("python");
    cmd.arg(&server_py).current_dir(&root);
    cmd.spawn()
        .map_err(|e| format!("Failed to start Harmony Cut service with python: {e}"))?;

    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if server_running().await {
            return Ok(health(app).await);
        }
    }
    Err("Harmony Cut service did not become ready on port 8765".to_string())
}

pub async fn prepare_project(
    input: HarmonyCutPrepareInput,
    app: Option<&tauri::AppHandle>,
) -> Result<HarmonyCutPrepareOutput, String> {
    start_service(app).await?;
    let client = client(3600)?;
    let payload = json!({
        "media_dir": input.media_dir,
        "title": input.title.unwrap_or_default(),
        "requirement": input.requirement.unwrap_or_default(),
        "auto_rotate_landscape": input.auto_rotate_landscape.unwrap_or(true),
    });
    let value = json_or_error(
        client
            .post(format!("{}/api/agent/prepare", server_url()))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Harmony Cut prepare request failed: {e}"))?,
        "Harmony Cut prepare",
    )
    .await?;
    let project = value.get("project").cloned().unwrap_or_else(|| json!({}));
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "Harmony Cut prepare response did not include project.id".to_string())?
        .to_string();
    Ok(HarmonyCutPrepareOutput {
        project_id: project_id.clone(),
        review_url: value
            .get("review_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        workbuddy_url: workbuddy_project_url(&project_id),
        marker_files: value
            .get("marker_files")
            .cloned()
            .unwrap_or_else(|| json!({})),
        next_instruction: value
            .get("next_instruction")
            .and_then(Value::as_str)
            .unwrap_or("Save in the workbench, then return to chat and continue.")
            .to_string(),
        project,
    })
}

pub async fn continue_project(
    input: HarmonyCutContinueInput,
    app: Option<&tauri::AppHandle>,
) -> Result<HarmonyCutContinueOutput, String> {
    start_service(app).await?;
    let client = client(3600)?;
    let payload = json!({
        "project_id": input.project_id.unwrap_or_default(),
        "skip_preview": input.skip_preview.unwrap_or(false),
        "skip_draft": input.skip_draft.unwrap_or(false),
    });
    let value = json_or_error(
        client
            .post(format!("{}/api/agent/continue", server_url()))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Harmony Cut continue request failed: {e}"))?,
        "Harmony Cut continue",
    )
    .await?;
    let project = value.get("project").cloned().unwrap_or_else(|| json!({}));
    let project_id = project
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(HarmonyCutContinueOutput {
        project_id,
        preview: value.get("preview").cloned().unwrap_or(Value::Null),
        draft: value.get("draft").cloned().unwrap_or(Value::Null),
        marker_files: value
            .get("marker_files")
            .cloned()
            .unwrap_or_else(|| json!({})),
        project,
    })
}

pub async fn list_projects(
    app: Option<&tauri::AppHandle>,
) -> Result<Vec<HarmonyCutProjectSummary>, String> {
    start_service(app).await?;
    let client = client(30)?;
    let value = json_or_error(
        client
            .get(format!("{}/api/projects", server_url()))
            .send()
            .await
            .map_err(|e| format!("Harmony Cut projects request failed: {e}"))?,
        "Harmony Cut projects",
    )
    .await?;
    let projects = value
        .get("projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(projects
        .into_iter()
        .filter_map(|project| {
            let id = project.get("id")?.as_str()?.to_string();
            Some(HarmonyCutProjectSummary {
                review_url: format!("{}/?project={}", server_url(), id),
                id,
                title: project
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                media_dir: project
                    .get("media_dir")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                workflow_status: project
                    .get("workflow_status")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect())
}

pub async fn get_project(
    project_id: &str,
    app: Option<&tauri::AppHandle>,
) -> Result<Value, String> {
    start_service(app).await?;
    let client = client(30)?;
    let value = json_or_error(
        client
            .get(format!("{}/api/projects/{}", server_url(), project_id))
            .send()
            .await
            .map_err(|e| format!("Harmony Cut project request failed: {e}"))?,
        "Harmony Cut project",
    )
    .await?;
    Ok(value.get("project").cloned().unwrap_or_else(|| json!({})))
}
