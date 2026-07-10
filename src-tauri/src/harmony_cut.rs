use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_HARMONY_CUT_ROOT: &str = r"D:\harmony_cut";
pub const DEFAULT_HARMONY_CUT_URL: &str = "http://127.0.0.1:8765";

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

pub fn root_dir() -> PathBuf {
    std::env::var("WORKBUDDY_HARMONY_CUT_ROOT")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_HARMONY_CUT_ROOT))
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

pub async fn health() -> HarmonyCutHealth {
    let root = root_dir();
    let installed = root.join("server.py").exists();
    let server_running = if installed {
        server_running().await
    } else {
        false
    };
    let message = if !installed {
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

pub async fn start_service() -> Result<HarmonyCutHealth, String> {
    let root = root_dir();
    let server_py = root.join("server.py");
    if !server_py.exists() {
        return Err(format!(
            "Harmony Cut server.py not found: {}",
            server_py.display()
        ));
    }
    if server_running().await {
        return Ok(health().await);
    }

    let mut cmd = piscis_kernel::proc::std_command("python");
    cmd.arg(&server_py).current_dir(&root);
    cmd.spawn()
        .map_err(|e| format!("Failed to start Harmony Cut service with python: {e}"))?;

    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if server_running().await {
            return Ok(health().await);
        }
    }
    Err("Harmony Cut service did not become ready on port 8765".to_string())
}

pub async fn prepare_project(
    input: HarmonyCutPrepareInput,
) -> Result<HarmonyCutPrepareOutput, String> {
    start_service().await?;
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
) -> Result<HarmonyCutContinueOutput, String> {
    start_service().await?;
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

pub async fn list_projects() -> Result<Vec<HarmonyCutProjectSummary>, String> {
    start_service().await?;
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

pub async fn get_project(project_id: &str) -> Result<Value, String> {
    start_service().await?;
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
