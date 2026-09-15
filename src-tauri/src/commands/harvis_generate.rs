//! Harmony media uses Responses background tasks, independently of the chat provider.
use crate::service_urls::{HARMONY_API_BASE, HARMONY_SERVER};
use crate::store::{settings::Settings, AppState};
use base64::Engine;
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use tauri::State;
use tokio::io::AsyncWriteExt;

fn harmony_api_key(settings: &Settings) -> Result<String, String> {
    let key = settings.harmony_api_key.trim();
    if key.is_empty() { Err("请先在设置中配置 Harmony API Key".into()) } else { Ok(key.into()) }
}
fn value_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())
}
async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() { return Err(format!("Harmony 请求失败（{status}）：{text}")); }
    serde_json::from_str(&text).map_err(|e| format!("Harmony 响应解析失败：{e}"))
}
fn normalize_status(raw: &str) -> &'static str {
    match raw.to_ascii_lowercase().as_str() {
        "success" | "succeeded" | "completed" | "complete" | "done" => "succeeded",
        "failed" | "failure" | "error" | "cancelled" | "canceled" | "incomplete" | "unsuccessful" => "failed",
        _ => "processing",
    }
}
fn media_url(value: &str) -> Option<String> {
    let value = value.trim().replace("&amp;", "&").replace("&#38;", "&");
    if value.starts_with("data:image/") || value.starts_with("data:audio/") || value.starts_with("data:video/") {
        return Some(value);
    }
    reqwest::Url::parse(&value).ok().filter(|url| url.scheme() == "https" && url.username().is_empty() && url.password().is_none()).map(|_| value)
}
// Extract media without rendering vendor HTML in the desktop webview.
fn extract_media(value: &Value, kind: &str) -> Option<String> {
    if let Some(items) = value.as_array() { return items.iter().find_map(|item| extract_media(item, kind)); }
    let keys: &[&str] = match kind {
        "image" => &["image_url", "imageUrl", "cld2ImageUrl", "result_url", "url"],
        "music" => &["audio_url", "audioUrl", "cld2AudioUrl", "mp3_url", "result_url", "url"],
        _ => &["video_url", "videoUrl", "cld2VideoUrl", "result_url", "url"],
    };
    for key in keys {
        if let Some(url) = value_str(value, key).and_then(media_url) { return Some(url); }
    }
    if let Some(text) = value_str(value, "text") {
        let tag = match kind { "image" => "img", "music" => "audio", _ => "video" };
        let pattern = format!(r#"(?i)<{tag}\b[^>]*\bsrc\s*=\s*["']([^"']+)["']"#);
        if let Some(url) = regex::Regex::new(&pattern).ok()?.captures(text).and_then(|c| media_url(&c[1])) { return Some(url); }
    }
    for key in ["output", "content", "data", "items", "artifacts"] {
        if let Some(url) = value.get(key).and_then(|item| extract_media(item, kind)) { return Some(url); }
    }
    None
}
fn task_result(response: &Value, kind: &str, task_id: &str) -> Value {
    let data = response.get("data").filter(|v| v.is_object()).unwrap_or(response);
    let mut status = normalize_status(value_str(response, "status").or_else(|| value_str(data, "status")).unwrap_or(""));
    let result_url = extract_media(response, kind);
    let mut error = response.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_string();
    if status == "succeeded" && result_url.is_none() {
        status = "failed";
        error = "渠道已结束任务，但没有返回可用成品；请联系管理员检查任务结果（无需重复提交）".into();
    }
    json!({ "kind": kind, "task_id": task_id, "status": status,
        "progress": if status == "succeeded" { 100 } else { 0 },
        "result_url": result_url, "error_message": error })
}
fn generation_body(model: &str, kind: &str, request: &Value) -> Result<Value, String> {
    let prompt = value_str(request, "prompt").ok_or("请填写生成提示词")?;
    let mut body = json!({"model": model, "input": prompt, "background": true});
    if kind == "music" {
        body["mvVersion"] = json!("chirp-v5-5");
        body["makeInstrumental"] = json!(true);
        body["title"] = json!(value_str(request, "title").unwrap_or("Harvis Music"));
        body["tags"] = json!(prompt);
        body["customMode"] = json!(true);
        body["inputType"] = json!("20");
        body["prompt"] = json!("[Instrumental]");
    } else if kind == "image" {
        let references = request.get("referenceUrls").and_then(Value::as_array).cloned().unwrap_or_default();
        if value_str(request, "mode") == Some("image") && references.is_empty() {
            return Err("参考图已失效，请重新添加参考图后生成".into());
        }
        if references.len() > 4 || references.iter().any(|v| v.as_str().and_then(media_url).is_none()) {
            return Err("参考图地址无效，最多可使用 4 张参考图".into());
        }
        body["images"] = json!(references);
        let width = request.get("outputWidth").and_then(Value::as_u64).unwrap_or(1024);
        let height = request.get("outputHeight").and_then(Value::as_u64).unwrap_or(1024);
        if width == 0 || height == 0 || width > 8192 || height > 8192 { return Err("图片尺寸须在 1 至 8192 像素之间".into()); }
        body["size"] = json!(format!("{width}x{height}"));
    }
    if let Some(duration) = request.get("duration") { body["duration"] = duration.clone(); }
    Ok(body)
}
#[tauri::command]
pub async fn harvis_generate(state: State<'_, AppState>, request: Value) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let kind = value_str(&request, "kind").ok_or("缺少生成任务类型")?;
    let model = match kind {
        "image" => &settings.harvis_image_model, "music" => &settings.harvis_music_model,
        "video" => &settings.harvis_video_model, _ => return Err("暂不支持该创作类型".into()),
    };
    if model.trim().is_empty() { return Err(format!("请先在设置中选择 Harvis {kind} 模型")); }
    let body = generation_body(model.trim(), kind, &request)?;
    let response = client()?.post(format!("{HARMONY_API_BASE}/responses"))
        .bearer_auth(harmony_api_key(&settings)?).json(&body).send().await.map_err(|e| e.to_string())?;
    let response = response_json(response).await?;
    let id = value_str(&response, "id").ok_or("渠道未返回任务 ID，无法追踪生成进度")?;
    Ok(task_result(&response, kind, id))
}
async fn query_task(settings: &Settings, task_id: &str, kind: &str) -> Result<Value, String> {
    if !["image", "music", "video"].contains(&kind) { return Err("不支持的任务类型".into()); }
    let id = urlencoding::encode(task_id);
    let url = if task_id.starts_with("resp_") { format!("{HARMONY_API_BASE}/responses/{id}") } else {
        // Old saved task IDs remain readable; only new submissions use Responses.
        match kind {
            "music" => format!("{HARMONY_SERVER}/suno/fetch/{id}"),
            "video" => format!("{HARMONY_API_BASE}/videos/{id}"),
            _ => return Err("旧图片任务没有可查询的异步 ID，请查看已有成品".into()),
        }
    };
    let response = client()?.get(url).bearer_auth(harmony_api_key(settings)?).send().await.map_err(|e| e.to_string())?;
    Ok(task_result(&response_json(response).await?, kind, task_id))
}
#[tauri::command]
pub async fn harvis_query_task(state: State<'_, AppState>, task_id: String, kind: String) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    query_task(&settings, &task_id, &kind).await
}
#[tauri::command]
pub async fn harvis_query_task_content(state: State<'_, AppState>, task_id: String, kind: String) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    query_task(&settings, &task_id, &kind).await
}
async fn save_result(result_url: &str, target: &Path) -> Result<Value, String> {
    const LIMIT: usize = 256 * 1024 * 1024;
    if !target.is_absolute() || target.file_name().is_none() || !target.parent().is_some_and(Path::is_dir) {
        return Err("请选择有效的完整文件保存路径".into());
    }
    let url = media_url(result_url).ok_or("不支持的下载地址，仅接受 HTTPS 或媒体数据")?;
    let temp = target.with_file_name(format!(".harvis-{}.part", uuid::Uuid::new_v4()));
    let result: Result<Value, String> = async {
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temp).await.map_err(|e| e.to_string())?;
        let mut size = 0usize;
        if url.starts_with("data:") {
            let (header, encoded) = url.split_once(',').ok_or("成品数据格式无效")?;
            if !header.ends_with(";base64") || encoded.len() > LIMIT * 4 / 3 + 4 { return Err("成品数据无效或超过 256MB 限制".into()); }
            let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|e| e.to_string())?;
            size = bytes.len();
            file.write_all(&bytes).await.map_err(|e| e.to_string())?;
        } else {
            // CDN URLs carry their own authorization. Never forward account keys.
            let mut response = client()?.get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
            let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
            if content_type.contains("text/html") || content_type.contains("application/json") { return Err("下载地址未返回媒体文件，链接可能已过期".into()); }
            if response.content_length().is_some_and(|n| n > LIMIT as u64) { return Err("成品超过 256MB 下载限制".into()); }
            while let Some(bytes) = response.chunk().await.map_err(|e| e.to_string())? {
                size += bytes.len();
                if size > LIMIT { return Err("成品超过 256MB 下载限制".into()); }
                file.write_all(&bytes).await.map_err(|e| e.to_string())?;
            }
        }
        if size == 0 { return Err("下载返回了空文件，请稍后重试".into()); }
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        if target.exists() {
            tokio::fs::copy(&temp, target).await.map_err(|e| format!("无法覆盖所选文件：{e}"))?;
            tokio::fs::remove_file(&temp).await.map_err(|e| e.to_string())?;
        } else {
            tokio::fs::rename(&temp, target).await.map_err(|e| format!("无法保存文件：{e}"))?;
        }
        Ok(json!({"localPath": target.to_string_lossy(), "sizeBytes": size}))
    }.await;
    if result.is_err() { let _ = tokio::fs::remove_file(&temp).await; }
    result
}
#[tauri::command]
pub async fn harvis_save_result(result_url: String, target_path: String) -> Result<Value, String> {
    save_result(&result_url, Path::new(&target_path)).await
}
#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn key_is_independent_of_chat_provider() {
        let mut settings = Settings::default();
        settings.provider = "deepseek".into();
        settings.deepseek_api_key = "other-key".into();
        settings.harmony_api_key = "harmony-key".into();
        assert_eq!(harmony_api_key(&settings).unwrap(), "harmony-key");
        settings.harmony_api_key.clear();
        assert!(harmony_api_key(&settings).is_err());
    }
    #[test]
    fn completed_requires_actual_media_and_kind_matches() {
        let response = json!({"status":"completed","output":[{"content":[{"type":"output_text","text":r#"<audio controls src="https://cdn.example/a.m4a?a=1&amp;b=2"></audio>"#}]}]});
        assert_eq!(extract_media(&response, "music").unwrap(), "https://cdn.example/a.m4a?a=1&b=2");
        assert!(extract_media(&response, "image").is_none());
        assert_eq!(task_result(&response, "music", "resp_1")["status"], "succeeded");
        assert_eq!(task_result(&response, "image", "resp_1")["status"], "failed");
        assert_eq!(normalize_status("incomplete"), "failed");
        assert_eq!(normalize_status("unsuccessful"), "failed");
        assert!(media_url("javascript:alert(1)").is_none());
    }
    #[test]
    fn submit_uses_background_and_preserves_references() {
        let body = generation_body("gpt-image-2", "image", &json!({"prompt":"test", "mode":"image", "referenceUrls":["https://cdn.example/ref.png"], "outputWidth":420, "outputHeight":592})).unwrap();
        assert_eq!(body["background"], true);
        assert_eq!(body["size"], "420x592");
        assert_eq!(body["images"][0], "https://cdn.example/ref.png");
        assert!(generation_body("gpt-image-2", "image", &json!({"prompt":"test", "mode":"image"})).is_err());
        let music = generation_body("suno_music", "music", &json!({"prompt":"piano"})).unwrap();
        assert_eq!(music["makeInstrumental"], true);
        assert_eq!(music["mvVersion"], "chirp-v5-5");
    }
    #[tokio::test]
    async fn download_saves_bytes_and_preserves_existing_file_on_error() {
        let folder = std::env::temp_dir().join(format!("harvis-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let target = folder.join("test.png");
        save_result("data:image/png;base64,aGVsbG8=", &target).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        save_result("data:image/png;base64,d29ybGQ=", &target).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"world");
        assert!(save_result("data:image/png;base64,", &target).await.is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"world");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        assert!(save_result("data:image/png;base64,aA==", Path::new("relative.png")).await.is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
