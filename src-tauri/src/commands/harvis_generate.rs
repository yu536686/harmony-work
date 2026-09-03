//! Harvis 创作模型调用（走 Harmony 内部网关）。
//!
//! Harmony 网关的生成任务（生图 / 视频 / 音乐 / TTS）统一走：
//! - 创建任务：`POST /v1/videos`（body: `{ "model": "...", "prompt": "...", ... }`）
//! - 查询状态：`GET /v1/videos/<task_id>`
//! - 查询内容：`GET /v1/videos/<task_id>/content`

use crate::store::settings::Settings;
use crate::store::AppState;
use serde_json::{json, Value};
use tauri::State;

const HARMONY_BASE: &str = "https://api.harmonywatch.cn:13001/v1";

fn harvis_model_for(settings: &Settings, kind: &str) -> Option<String> {
    let model = match kind {
        "image" => settings.harvis_image_model.trim(),
        "video" => settings.harvis_video_model.trim(),
        "music" => settings.harvis_music_model.trim(),
        "tts" => settings.harvis_tts_model.trim(),
        _ => "",
    };
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

fn harmony_api_key(settings: &Settings) -> Result<String, String> {
    let key = settings.active_api_key().trim().to_string();
    if key.is_empty() {
        Err("请先在设置中配置 Harmony API Key".to_string())
    } else {
        Ok(key)
    }
}

async fn get_json(url: String, api_key: &str) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format!("请求失败（status={status}）：{text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("响应解析失败：{e}"))
}

/// 创建 Harvis 生成任务（生图 / 视频 / 音乐 / TTS）。
/// 请求体需包含 `kind`（image/video/music/tts）与 `prompt` 等参数；
/// 实际使用的模型由设置中对应的 Harvis 模型选择决定。
#[tauri::command]
pub async fn harvis_generate(
    state: State<'_, AppState>,
    request: Value,
) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let kind = request
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if kind.is_empty() {
        return Err("缺少生成任务类型 kind（image/video/music/tts）".to_string());
    }
    let model = harvis_model_for(&settings, kind)
        .ok_or_else(|| format!("请先在设置中选择 Harvis {kind} 模型"))?;
    let api_key = harmony_api_key(&settings)?;

    let mut body = match request.as_object() {
        Some(obj) => obj.clone(),
        None => return Err("请求体必须是 JSON 对象".to_string()),
    };
    body.insert("model".to_string(), json!(model));

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{HARMONY_BASE}/videos"))
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("创建任务失败：{e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取响应失败：{e}"))?;
    if !status.is_success() {
        return Err(format!("创建任务失败（status={status}）：{text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("响应解析失败：{e}"))
}

/// 查询 Harvis 生成任务状态。
#[tauri::command]
pub async fn harvis_query_task(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let api_key = harmony_api_key(&settings)?;
    get_json(
        format!("{HARMONY_BASE}/videos/{}", urlencoding::encode(&task_id)),
        &api_key,
    )
    .await
}

/// 查询 Harvis 生成任务内容（音频 / 图片等结果）。
#[tauri::command]
pub async fn harvis_query_task_content(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let api_key = harmony_api_key(&settings)?;
    get_json(
        format!(
            "{HARMONY_BASE}/videos/{}/content",
            urlencoding::encode(&task_id)
        ),
        &api_key,
    )
    .await
}
