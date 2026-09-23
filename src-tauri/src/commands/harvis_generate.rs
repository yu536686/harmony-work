//! Harmony media uses Responses background tasks, independently of the chat provider.
use crate::service_urls::{HARMONY_API_BASE, HARMONY_SERVER};
use crate::harvis_responses::{generation_body, media_url, task_result};
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
#[tauri::command]
pub async fn harvis_preview_result(result_url: String, kind: String) -> Result<Value, String> {
    let expected = match kind.as_str() {
        "image" => "image/",
        "music" => "audio/",
        _ => return Err("不支持预览此类成品".into()),
    };
    let limit = if kind == "image" { 16 * 1024 * 1024 } else { 32 * 1024 * 1024 };
    let url = media_url(&result_url).ok_or("成品地址无效或已过期")?;
    if url.starts_with("data:") {
        let (header, body) = url.split_once(',').ok_or("成品数据格式无效")?;
        if !header.starts_with(&format!("data:{expected}")) || !header.ends_with(";base64") || body.len() > limit * 4 / 3 + 4 {
            return Err("成品类型无效或超过预览大小限制".into());
        }
        return Ok(json!({"dataUrl": url}));
    }
    let mut response = client()?.get(url).send().await.map_err(|e| e.to_string())?
        .error_for_status().map_err(|e| e.to_string())?;
    let mime = response.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok()).unwrap_or("")
        .split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if !mime.starts_with(expected) {
        return Err("成品链接没有返回可预览的媒体文件".into());
    }
    if response.content_length().is_some_and(|size| size > limit as u64) {
        return Err("成品超过预览大小限制，请下载查看".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > limit {
            return Err("成品超过预览大小限制，请下载查看".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() { return Err("成品内容为空".into()); }
    Ok(json!({"dataUrl": format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))}))
}
#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::harvis_responses::{extract_media, normalize_status};
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
