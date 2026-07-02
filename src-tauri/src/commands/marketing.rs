//! Marketing portal integration.
//!
//! WorkBuddy keeps the marketing portal service on the public marketing host as the
//! source of truth for users, asset indexes, and campaign metrics. These
//! commands proxy authenticated requests from the desktop app so the WebView
//! does not have to fight CORS or browser cookie rules.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::sync::Mutex;

const DEFAULT_MARKETING_BASE: &str = "http://61.150.31.150:8090";

static MARKETING_COOKIE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn cookie_store() -> &'static Mutex<Option<String>> {
    MARKETING_COOKIE.get_or_init(|| Mutex::new(None))
}

fn marketing_cookie_path() -> PathBuf {
    crate::app::logging::default_app_data_dir().join("marketing.session.cookie")
}

async fn load_persisted_cookie() -> Option<String> {
    let path = marketing_cookie_path();
    let value = tokio::fs::read_to_string(path).await.ok()?;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

async fn persist_cookie(cookie: &str) {
    let path = marketing_cookie_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(path, cookie).await;
}

async fn clear_persisted_cookie() {
    let _ = tokio::fs::remove_file(marketing_cookie_path()).await;
}

fn marketing_base_url() -> String {
    std::env::var("WORKBUDDY_MARKETING_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_MARKETING_BASE.to_string())
}

fn marketing_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Xiaoheng-WorkBuddy/0.7 Marketing")
        .build()
        .map_err(|e| e.to_string())
}

async fn apply_cookie(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let cookie = cookie_store().lock().await.clone();
    let cookie = if cookie.is_some() {
        cookie
    } else {
        let persisted = load_persisted_cookie().await;
        if persisted.is_some() {
            *cookie_store().lock().await = persisted.clone();
        }
        persisted
    };
    if let Some(cookie) = cookie {
        req.header(COOKIE, cookie)
    } else {
        req
    }
}

async fn store_response_cookie(resp: &reqwest::Response) {
    let cookies = resp
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !cookies.is_empty() {
        let cookie = cookies.join("; ");
        *cookie_store().lock().await = Some(cookie.clone());
        persist_cookie(&cookie).await;
    }
}

async fn json_or_error(resp: reqwest::Response, label: &str) -> Result<Value, String> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("{label} returned HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("{label} response is not valid JSON: {e}; body={text}"))
}

fn harmony_workspace_default() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("harmony")
        .to_string_lossy()
        .to_string()
}

fn safe_file_name(name: &str) -> String {
    let cleaned = name
        .trim()
        .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    if cleaned.is_empty() {
        "marketing-asset".to_string()
    } else {
        cleaned
    }
}

fn media_type_for_name(name: &str, fallback: Option<&str>) -> String {
    if let Some(value) = fallback.filter(|value| !value.trim().is_empty()) {
        return value.to_string();
    }
    match Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "pdf" => "application/pdf",
        "txt" | "md" | "csv" | "json" => "text/plain",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn filename_from_disposition(value: Option<&str>) -> Option<String> {
    let value = value?;
    for part in value.split(';').map(str::trim) {
        if let Some(raw) = part.strip_prefix("filename*=UTF-8''") {
            return urlencoding::decode(raw).ok().map(|value| value.to_string());
        }
        if let Some(raw) = part.strip_prefix("filename=") {
            return Some(raw.trim_matches('"').to_string());
        }
    }
    None
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingLoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingAssetQuery {
    folder: Option<String>,
    asset_type: Option<String>,
    extension: Option<String>,
    format_group: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingPrepareAssetRequest {
    asset_id: String,
    file_name: Option<String>,
    workspace_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingAssetDataUrlRequest {
    asset_id: String,
    file_name: Option<String>,
    max_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingPreparedAsset {
    asset_id: String,
    file_name: String,
    media_type: String,
    local_path: String,
    source_url: String,
    byte_size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingAssetDataUrl {
    asset_id: String,
    file_name: String,
    media_type: String,
    data_url: String,
    byte_size: u64,
}

struct DownloadedMarketingAsset {
    file_name: String,
    media_type: String,
    source_url: String,
    bytes: Vec<u8>,
}

#[tauri::command]
pub async fn marketing_login(request: MarketingLoginRequest) -> Result<Value, String> {
    let client = marketing_client()?;
    let resp = client
        .post(format!("{}/api/auth/login", marketing_base_url()))
        .json(&json!({"email": request.email, "password": request.password}))
        .send()
        .await
        .map_err(|e| format!("Marketing login request failed: {e}"))?;
    store_response_cookie(&resp).await;
    json_or_error(resp, "Marketing login").await
}

#[tauri::command]
pub async fn marketing_logout() -> Result<Value, String> {
    let client = marketing_client()?;
    let req = apply_cookie(client.post(format!("{}/api/auth/logout", marketing_base_url()))).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing logout request failed: {e}"))?;
    *cookie_store().lock().await = None;
    clear_persisted_cookie().await;
    json_or_error(resp, "Marketing logout").await
}

#[tauri::command]
pub async fn marketing_me() -> Result<Value, String> {
    let client = marketing_client()?;
    let req = apply_cookie(client.get(format!("{}/api/auth/me", marketing_base_url()))).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing me request failed: {e}"))?;
    store_response_cookie(&resp).await;
    json_or_error(resp, "Marketing me").await
}

#[tauri::command]
pub async fn marketing_asset_summary() -> Result<Value, String> {
    marketing_get_json("/api/assets/summary", Vec::new()).await
}

#[tauri::command]
pub async fn marketing_asset_folders() -> Result<Value, String> {
    marketing_get_json("/api/assets/folders", Vec::new()).await
}

#[tauri::command]
pub async fn marketing_asset_formats() -> Result<Value, String> {
    marketing_get_json("/api/assets/formats", Vec::new()).await
}

#[tauri::command]
pub async fn marketing_assets(query: MarketingAssetQuery) -> Result<Value, String> {
    let mut params = Vec::new();
    if let Some(value) = query.folder.filter(|v| !v.trim().is_empty()) {
        params.push(("folder".to_string(), value));
    }
    if let Some(value) = query.asset_type.filter(|v| !v.trim().is_empty()) {
        params.push(("type".to_string(), value));
    }
    if let Some(value) = query.extension.filter(|v| !v.trim().is_empty()) {
        params.push(("extension".to_string(), value));
    }
    if let Some(value) = query.format_group.filter(|v| !v.trim().is_empty()) {
        params.push(("formatGroup".to_string(), value));
    }
    if let Some(value) = query.q.filter(|v| !v.trim().is_empty()) {
        params.push(("q".to_string(), value));
    }
    if let Some(value) = query.sort.filter(|v| !v.trim().is_empty()) {
        params.push(("sort".to_string(), value));
    }
    params.push(("limit".to_string(), query.limit.unwrap_or(36).min(120).to_string()));
    params.push(("offset".to_string(), query.offset.unwrap_or(0).to_string()));
    marketing_get_json("/api/assets", params).await
}

#[tauri::command]
pub async fn marketing_campaign_summary(minutes: Option<u32>, platform: Option<String>) -> Result<Value, String> {
    let mut params = vec![(
        "minutes".to_string(),
        minutes.unwrap_or(1440).clamp(10, 10080).to_string(),
    )];
    if let Some(value) = platform.filter(|value| !value.trim().is_empty()) {
        params.push(("platform".to_string(), value));
    }
    marketing_get_json("/api/campaign/summary", params).await
}

#[tauri::command]
pub async fn marketing_douyin_accounts() -> Result<Value, String> {
    marketing_get_json("/api/douyin/accounts", Vec::new()).await
}

async fn marketing_get_json(path: &str, params: Vec<(String, String)>) -> Result<Value, String> {
    let client = marketing_client()?;
    let url = format!("{}{}", marketing_base_url(), path);
    let req = apply_cookie(client.get(url).query(&params)).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing request failed: {e}"))?;
    store_response_cookie(&resp).await;
    json_or_error(resp, "Marketing API").await
}

async fn download_marketing_asset(
    asset_id: &str,
    requested_file_name: Option<&str>,
) -> Result<DownloadedMarketingAsset, String> {
    let client = marketing_client()?;
    let source_url = format!(
        "{}/api/assets/{}/file",
        marketing_base_url(),
        urlencoding::encode(asset_id)
    );
    let req = apply_cookie(client.get(&source_url)).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing asset download failed: {e}"))?;
    store_response_cookie(&resp).await;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Marketing asset download returned HTTP {status}: {body}"));
    }
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let disposition_name = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| filename_from_disposition(Some(value)));
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Failed to read Marketing asset bytes: {e}"))?
        .to_vec();
    let file_name = safe_file_name(
        requested_file_name
            .or(disposition_name.as_deref())
            .unwrap_or("marketing-asset"),
    );
    let media_type = media_type_for_name(&file_name, content_type.as_deref());
    Ok(DownloadedMarketingAsset {
        file_name,
        media_type,
        source_url,
        bytes,
    })
}

#[tauri::command]
pub async fn marketing_asset_data_url(
    request: MarketingAssetDataUrlRequest,
) -> Result<MarketingAssetDataUrl, String> {
    let asset = download_marketing_asset(&request.asset_id, request.file_name.as_deref()).await?;
    let max_bytes = request
        .max_bytes
        .unwrap_or(12 * 1024 * 1024)
        .clamp(128 * 1024, 40 * 1024 * 1024);
    if asset.bytes.len() as u64 > max_bytes {
        return Err(format!(
            "素材过大，无法内联预览（{} bytes > {} bytes）。请使用“打开文件”。",
            asset.bytes.len(),
            max_bytes
        ));
    }
    let encoded = BASE64_STANDARD.encode(&asset.bytes);
    Ok(MarketingAssetDataUrl {
        asset_id: request.asset_id,
        file_name: asset.file_name,
        media_type: asset.media_type.clone(),
        data_url: format!("data:{};base64,{}", asset.media_type, encoded),
        byte_size: asset.bytes.len() as u64,
    })
}

#[tauri::command]
pub async fn marketing_prepare_asset_for_chat(
    request: MarketingPrepareAssetRequest,
) -> Result<MarketingPreparedAsset, String> {
    let asset = download_marketing_asset(&request.asset_id, request.file_name.as_deref()).await?;
    let workspace = request
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(harmony_workspace_default()));
    let dir = workspace.join("marketing-assets");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Failed to create marketing asset cache dir: {e}"))?;
    let local_path = dir.join(&asset.file_name);
    tokio::fs::write(&local_path, &asset.bytes)
        .await
        .map_err(|e| format!("Failed to save marketing asset: {e}"))?;
    Ok(MarketingPreparedAsset {
        asset_id: request.asset_id,
        file_name: asset.file_name,
        media_type: asset.media_type,
        local_path: local_path.to_string_lossy().to_string(),
        source_url: asset.source_url,
        byte_size: asset.bytes.len() as u64,
    })
}
