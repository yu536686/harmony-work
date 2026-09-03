//! Harvis 创作模型调用（走 Harmony 内部网关）。
//!
//! Harmony 网关按媒体类型路由到不同上游，不能统一走 `/v1/videos`：
//! - 生图：`POST /v1/images/generations`（OpenAI 图像格式，同步返回成品）
//! - 视频：`POST /v1/videos`（OpenAI Sora 兼容格式，异步任务）
//! - 音乐：`POST /suno/submit/music`（Suno 代理，异步任务，根路径不带 /v1）
//! - TTS：暂未开放，预留接口

use crate::store::settings::Settings;
use crate::store::AppState;
use serde_json::{json, Value};
use tauri::State;

const HARMONY_SERVER: &str = "https://api.harmonywatch.cn:13001";
const HARMONY_V1: &str = "https://api.harmonywatch.cn:13001/v1";

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

fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
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

async fn post_json(url: String, api_key: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(body)
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
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("响应解析失败：{e}"))
}

fn normalize_status(raw: &str) -> &'static str {
    let value = raw.to_lowercase();
    if value.contains("success") || value.contains("complete") || value == "done" || value == "succeeded" {
        "succeeded"
    } else if value.contains("fail") || value.contains("error") {
        "failed"
    } else {
        "processing"
    }
}

fn extract_image_url(resp: &Value) -> Option<String> {
    let first = resp.get("data")?.as_array()?.first()?;
    if let Some(url) = value_str(first, "url") {
        return Some(url.to_string());
    }
    if let Some(b64) = value_str(first, "b64_json") {
        return Some(format!("data:image/png;base64,{b64}"));
    }
    None
}

fn find_audio_url(value: &Value) -> Option<String> {
    if let Some(arr) = value.as_array() {
        return arr.iter().find_map(find_audio_url);
    }
    for key in ["audio_url", "audioUrl", "url", "mp3_url"] {
        if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn extract_music_url(resp: &Value) -> Option<String> {
    let task = resp.get("data")?;
    find_audio_url(task.get("data")?)
}

fn music_progress(data: Option<&Value>) -> i64 {
    data.and_then(|d| d.get("progress"))
        .and_then(|p| {
            if let Some(n) = p.as_i64() {
                return Some(n);
            }
            if let Some(s) = p.as_str() {
                return s.trim().trim_end_matches('%').parse::<i64>().ok();
            }
            None
        })
        .unwrap_or(0)
}

fn image_size_from(request: &Value) -> Option<String> {
    let preset = value_str(request, "canvasPreset").unwrap_or("");
    match preset {
        "television" | "phone-fullscreen" => Some("1536x1024".to_string()),
        "vertical" | "store-poster" => Some("1024x1536".to_string()),
        "custom" => {
            let width = request.get("outputWidth").and_then(|v| v.as_u64());
            let height = request.get("outputHeight").and_then(|v| v.as_u64());
            match (width, height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => Some(format!("{w}x{h}")),
                _ => Some("1024x1024".to_string()),
            }
        }
        _ => Some("1024x1024".to_string()),
    }
}

async fn generate_image(model: &str, api_key: &str, request: &Value) -> Result<Value, String> {
    let prompt = value_str(request, "prompt").unwrap_or("").to_string();
    if prompt.is_empty() {
        return Err("缺少图片生成提示词 prompt".to_string());
    }
    let mut body = serde_json::Map::new();
    body.insert("model".to_string(), json!(model));
    body.insert("prompt".to_string(), json!(prompt));
    body.insert("n".to_string(), json!(1));
    if let Some(size) = image_size_from(request) {
        body.insert("size".to_string(), json!(size));
    }
    let resp = post_json(
        format!("{HARMONY_V1}/images/generations"),
        api_key,
        &Value::Object(body),
    )
    .await?;
    let result_url = extract_image_url(&resp);
    Ok(json!({
        "kind": "image",
        "task_id": Value::Null,
        "status": "succeeded",
        "result_url": result_url,
        "raw": resp,
    }))
}

async fn generate_video(model: &str, api_key: &str, request: &Value) -> Result<Value, String> {
    let mut body = match request.as_object() {
        Some(obj) => obj.clone(),
        None => return Err("请求体必须是 JSON 对象".to_string()),
    };
    body.remove("kind");
    body.insert("model".to_string(), json!(model));
    let resp = post_json(format!("{HARMONY_V1}/videos"), api_key, &Value::Object(body)).await?;
    let task_id = value_str(&resp, "task_id")
        .or_else(|| value_str(&resp, "id"))
        .unwrap_or("")
        .to_string();
    Ok(json!({
        "kind": "video",
        "task_id": task_id,
        "status": if task_id.is_empty() { "failed" } else { "submitted" },
        "result_url": Value::Null,
        "raw": resp,
    }))
}

async fn generate_music(model: &str, api_key: &str, request: &Value) -> Result<Value, String> {
    let prompt = value_str(request, "prompt").unwrap_or("").to_string();
    if prompt.is_empty() {
        return Err("缺少音乐生成提示词 prompt".to_string());
    }
    let title = value_str(request, "title").unwrap_or("Harvis Music").to_string();
    let mut body = serde_json::Map::new();
    body.insert("prompt".to_string(), json!(prompt));
    body.insert("title".to_string(), json!(title));
    if let Some(tags) = value_str(request, "tags") {
        body.insert("tags".to_string(), json!(tags));
    }
    if let Some(mv) = value_str(request, "mv") {
        body.insert("mv".to_string(), json!(mv));
    }
    // Suno 代理接口不使用 model 字段，按 Suno 约定组装请求体。
    let _ = model;
    let resp = post_json(
        format!("{HARMONY_SERVER}/suno/submit/music"),
        api_key,
        &Value::Object(body),
    )
    .await?;
    // Suno 创建任务响应：{"code":"success","data":"<task_id>"}
    let task_id = value_str(&resp, "data")
        .or_else(|| value_str(&resp, "task_id"))
        .or_else(|| value_str(&resp, "id"))
        .unwrap_or("")
        .to_string();
    if task_id.is_empty() {
        return Err(format!("音乐任务创建失败，网关未返回任务 ID：{resp}"));
    }
    Ok(json!({
        "kind": "music",
        "task_id": task_id,
        "status": "submitted",
        "result_url": Value::Null,
        "raw": resp,
    }))
}

async fn query_video(api_key: &str, task_id: &str) -> Result<Value, String> {
    let url = format!("{HARMONY_V1}/videos/{}", urlencoding::encode(task_id));
    let resp = get_json(url, api_key).await?;
    let status = normalize_status(value_str(&resp, "status").unwrap_or(""));
    let progress = resp.get("progress").and_then(|v| v.as_i64()).unwrap_or(0);
    let result_url = value_str(&resp, "url").map(|s| s.to_string());
    Ok(json!({
        "kind": "video",
        "task_id": task_id,
        "status": status,
        "progress": progress,
        "result_url": result_url,
        "raw": resp,
    }))
}

async fn query_music(api_key: &str, task_id: &str) -> Result<Value, String> {
    let url = format!("{HARMONY_SERVER}/suno/fetch/{}", urlencoding::encode(task_id));
    let resp = get_json(url, api_key).await?;
    let data = resp.get("data");
    let status = normalize_status(data.and_then(|d| value_str(d, "status")).unwrap_or(""));
    let progress = music_progress(data);
    let result_url = extract_music_url(&resp);
    Ok(json!({
        "kind": "music",
        "task_id": task_id,
        "status": status,
        "progress": progress,
        "result_url": result_url,
        "raw": resp,
    }))
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

    match kind {
        "image" => generate_image(&model, &api_key, &request).await,
        "video" => generate_video(&model, &api_key, &request).await,
        "music" => generate_music(&model, &api_key, &request).await,
        "tts" => Err("Harvis TTS 生成能力暂未开放，敬请期待".to_string()),
        _ => Err(format!("不支持的生成任务类型：{kind}")),
    }
}

/// 查询 Harvis 生成任务状态（生图 / 视频 / 音乐 / TTS）。
#[tauri::command]
pub async fn harvis_query_task(
    state: State<'_, AppState>,
    task_id: String,
    kind: String,
) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let api_key = harmony_api_key(&settings)?;
    match kind.trim() {
        "image" => Ok(json!({
            "kind": "image",
            "task_id": task_id,
            "status": "succeeded",
            "progress": 100,
            "result_url": Value::Null,
        })),
        "video" => query_video(&api_key, &task_id).await,
        "music" => query_music(&api_key, &task_id).await,
        "tts" => Err("Harvis TTS 生成能力暂未开放，敬请期待".to_string()),
        other => Err(format!("不支持的任务类型：{other}")),
    }
}

/// 查询 Harvis 生成任务内容（音频 / 图片等结果）。
#[tauri::command]
pub async fn harvis_query_task_content(
    state: State<'_, AppState>,
    task_id: String,
    kind: String,
) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let api_key = harmony_api_key(&settings)?;
    match kind.trim() {
        "image" => Ok(json!({
            "kind": "image",
            "task_id": task_id,
            "result_url": Value::Null,
        })),
        "video" => {
            let query = query_video(&api_key, &task_id).await?;
            let result_url = query.get("result_url").cloned().unwrap_or(Value::Null);
            Ok(json!({ "kind": "video", "task_id": task_id, "result_url": result_url }))
        }
        "music" => {
            let query = query_music(&api_key, &task_id).await?;
            let result_url = query.get("result_url").cloned().unwrap_or(Value::Null);
            Ok(json!({ "kind": "music", "task_id": task_id, "result_url": result_url }))
        }
        "tts" => Err("Harvis TTS 生成能力暂未开放，敬请期待".to_string()),
        other => Err(format!("不支持的任务类型：{other}")),
    }
}
