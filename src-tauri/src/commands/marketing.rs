//! Marketing portal integration.
//!
//! WorkBuddy keeps the marketing portal service on the public marketing host as the
//! source of truth for users, asset indexes, and campaign metrics. These
//! commands proxy authenticated requests from the desktop app so the WebView
//! does not have to fight CORS or browser cookie rules.

use crate::harvis_prompt::{compose_harvis_music_prompt, HarvisEvidenceCase, HarvisPromptInput};
use crate::store::secrets::SecretStore;
use crate::store::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::State;
use tokio::sync::Mutex;

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
        return None;
    }

    // Older releases stored this session value in plaintext. Prefer the local
    // SecretStore value, but keep that legacy format readable for one upgrade.
    let app_dir = crate::app::logging::default_app_data_dir();
    if let Ok(store) = SecretStore::new(&app_dir) {
        if let Ok(decrypted) = store.decrypt_hex(&value) {
            return Some(decrypted);
        }
    }
    Some(value)
}

async fn persist_cookie(cookie: &str) {
    let path = marketing_cookie_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let app_dir = crate::app::logging::default_app_data_dir();
    let value = SecretStore::new(&app_dir)
        .and_then(|store| store.encrypt_hex(cookie))
        .unwrap_or_else(|_| cookie.to_string());
    let _ = tokio::fs::write(path, value).await;
}

async fn clear_persisted_cookie() {
    let _ = tokio::fs::remove_file(marketing_cookie_path()).await;
}

pub(crate) fn marketing_base_url() -> String {
    crate::service_urls::marketing_base_url()
}

pub(crate) fn marketing_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Harmony-Work/0.7 Marketing")
        .build()
        .map_err(|e| e.to_string())
}

pub(crate) async fn apply_cookie(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
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
    serde_json::from_str(&text)
        .map_err(|e| format!("{label} response is not valid JSON: {e}; body={text}"))
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
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
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
    content_class: Option<String>,
    include_auxiliary: Option<bool>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingCreativeReferenceUploadRequest {
    file_name: String,
    media_type: String,
    data_base64: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingSaveAssetRequest {
    asset_id: String,
    file_name: Option<String>,
    target_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingCreativeMusicPromptRequest {
    business_line: String,
    brand: String,
    watch_collection: String,
    model: Option<String>,
    theme: Option<String>,
    watch_style: Option<String>,
    desired_style: Option<String>,
    bpm: Option<u32>,
    duration: u32,
    #[serde(default)]
    details: BTreeMap<String, String>,
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
    if let Some(value) = query.content_class.filter(|v| !v.trim().is_empty()) {
        params.push(("contentClass".to_string(), value));
    }
    params.push((
        "includeAuxiliary".to_string(),
        query.include_auxiliary.unwrap_or(false).to_string(),
    ));
    if let Some(value) = query.q.filter(|v| !v.trim().is_empty()) {
        params.push(("q".to_string(), value));
    }
    if let Some(value) = query.sort.filter(|v| !v.trim().is_empty()) {
        params.push(("sort".to_string(), value));
    }
    params.push((
        "limit".to_string(),
        query.limit.unwrap_or(36).min(120).to_string(),
    ));
    params.push(("offset".to_string(), query.offset.unwrap_or(0).to_string()));
    let mut result = marketing_get_json("/api/assets", params).await?;
    let transfer = marketing_transfer_ticket().await.ok();
    if let Some(assets) = result.get_mut("assets").and_then(Value::as_array_mut) {
        for asset in assets {
            let preview_url = transfer
                .as_ref()
                .and_then(|ticket| {
                    asset.get("id")
                        .and_then(Value::as_str)
                        .and_then(|id| direct_asset_url(ticket, id).ok())
                })
                .map(|url| url.to_string())
                .unwrap_or_default();
            asset["previewUrl"] = Value::String(preview_url);
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn marketing_campaign_summary(
    minutes: Option<u32>,
    platform: Option<String>,
) -> Result<Value, String> {
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketingAccountOverview {
    pub user: Value,
    pub balance: Option<f64>,
    pub usage: Option<Value>,
    pub gateway_api_key: Option<String>,
}

fn normalize_account_balance(value: &Value) -> Option<f64> {
    value.get("balance").and_then(Value::as_f64)
}

fn normalize_account_usage(value: &Value) -> Option<Value> {
    value.get("stat").filter(|v| !v.is_null()).cloned()
}

fn normalize_account_gateway_key(value: &Value) -> Option<String> {
    value
        .get("gatewayApiKey")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn build_account_overview(
    user: Value,
    balance: Option<Value>,
    usage: Option<Value>,
    credentials: Option<Value>,
) -> MarketingAccountOverview {
    MarketingAccountOverview {
        user,
        balance: balance.as_ref().and_then(normalize_account_balance),
        usage: usage.as_ref().and_then(normalize_account_usage),
        gateway_api_key: credentials
            .as_ref()
            .and_then(normalize_account_gateway_key),
    }
}

async fn fetch_marketing_json_optional(path: &str) -> Option<Value> {
    let client = marketing_client().ok()?;
    let req = apply_cookie(client.get(format!("{}{}", marketing_base_url(), path))).await;
    let resp = req.send().await.ok()?;
    store_response_cookie(&resp).await;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

/// 汇总门户账户、积分余额、网关用量和个人 Harmony API Key。
/// 身份接口失败（401/403）会清除过期会话并报错；非关键统计接口失败只降级为 `null`。
/// 该命令不持久化任何网关凭据，仅向当前调用返回结果。
#[tauri::command]
pub async fn marketing_account_overview() -> Result<MarketingAccountOverview, String> {
    let client = marketing_client()?;
    let req = apply_cookie(client.get(format!("{}/api/auth/me", marketing_base_url()))).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("账户信息请求失败：{e}"))?;
    store_response_cookie(&resp).await;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        clear_persisted_cookie().await;
        *cookie_store().lock().await = None;
        return Err(format!("登录已过期（HTTP {status}），请重新登录"));
    }
    if !status.is_success() {
        return Err(format!("账户信息获取失败（HTTP {status}）：{text}"));
    }
    let me_value: Value =
        serde_json::from_str(&text).map_err(|e| format!("账户信息解析失败：{e}"))?;
    let user = me_value.get("user").cloned().unwrap_or(Value::Null);

    let (balance, usage, credentials) = tokio::join!(
        fetch_marketing_json_optional("/api/gateway/balance"),
        fetch_marketing_json_optional("/api/gateway/usage"),
        fetch_marketing_json_optional("/api/gateway/credentials"),
    );

    Ok(build_account_overview(user, balance, usage, credentials))
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

async fn marketing_post_json(path: &str, body: Value) -> Result<Value, String> {
    let client = marketing_client()?;
    let url = format!("{}{}", marketing_base_url(), path);
    let req = apply_cookie(client.post(url).json(&body)).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing request failed: {e}"))?;
    store_response_cookie(&resp).await;
    json_or_error(resp, "Marketing API").await
}

#[tauri::command]
pub async fn marketing_creative_jobs() -> Result<Value, String> {
    marketing_get_json("/api/creative/jobs", Vec::new()).await
}

#[tauri::command]
pub async fn marketing_creative_music_config() -> Result<Value, String> {
    marketing_get_json("/api/creative/music/config", Vec::new()).await
}

#[tauri::command]
pub async fn marketing_creative_job(job_id: String) -> Result<Value, String> {
    marketing_get_json(
        &format!("/api/creative/jobs/{}", urlencoding::encode(&job_id)),
        Vec::new(),
    )
    .await
}

#[tauri::command]
pub async fn marketing_creative_create_music(request: Value) -> Result<Value, String> {
    marketing_post_json("/api/creative/music/jobs", request).await
}

fn compact_prompt_field(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn omit_null_object_fields(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, field| !field.is_null());
    }
    value
}

#[tauri::command]
pub async fn marketing_creative_compose_music_prompt(
    state: State<'_, AppState>,
    request: MarketingCreativeMusicPromptRequest,
) -> Result<Value, String> {
    let business_line = compact_prompt_field(&request.business_line, 80);
    let brand = compact_prompt_field(&request.brand, 80);
    let watch_collection = compact_prompt_field(&request.watch_collection, 100);
    if business_line.is_empty() || brand.is_empty() || watch_collection.is_empty() {
        return Err("业务线、品牌和腕表系列不能为空".to_string());
    }
    let duration = request.duration.clamp(10, 360);
    let details = request
        .details
        .iter()
        .filter_map(|(key, value)| {
            let key = compact_prompt_field(key, 48);
            let value = compact_prompt_field(value, 240);
            (!key.is_empty() && !value.is_empty()).then_some((key, value))
        })
        .collect::<BTreeMap<_, _>>();
    let context = marketing_post_json(
        "/api/creative/music/assist-context",
        omit_null_object_fields(json!({
            "businessLine": business_line,
            "brand": brand,
            "collection": watch_collection,
            "model": request.model.as_deref().map(|value| compact_prompt_field(value, 120)).filter(|value| !value.is_empty()),
            "theme": request.theme.as_deref().map(|value| compact_prompt_field(value, 180)).filter(|value| !value.is_empty()),
            "watchStyle": request.watch_style.as_deref().map(|value| compact_prompt_field(value, 80)).filter(|value| !value.is_empty()),
            "desiredStyle": request.desired_style.as_deref().map(|value| compact_prompt_field(value, 120)).filter(|value| !value.is_empty()),
            "details": details,
        })),
    )
    .await?;
    let evidence_cases = context
        .get("evidenceCases")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<HarvisEvidenceCase>>(value).ok())
        .unwrap_or_default();
    let settings = state.settings.lock().await.clone();
    let result = compose_harvis_music_prompt(
        &settings,
        HarvisPromptInput {
            business_line,
            brand,
            watch_collection,
            watch_style: request
                .watch_style
                .map(|value| compact_prompt_field(&value, 80)),
            desired_style: request
                .desired_style
                .map(|value| compact_prompt_field(&value, 120)),
            bpm: request.bpm.filter(|value| (40..=220).contains(value)),
            duration,
            campaign_context: details
                .iter()
                .map(|(key, value)| format!("{key}: {value}"))
                .collect(),
            evidence_cases,
        },
    )
    .await?;
    serde_json::to_value(result).map_err(|error| format!("提示词助手结果序列化失败：{error}"))
}

#[tauri::command]
pub async fn marketing_creative_create_image(request: Value) -> Result<Value, String> {
    marketing_post_json("/api/creative/image/jobs", request).await
}

#[tauri::command]
pub async fn marketing_creative_stage_for_chat(job_id: String) -> Result<Value, String> {
    marketing_post_json(
        &format!(
            "/api/creative/jobs/{}/stage-for-chat",
            urlencoding::encode(&job_id)
        ),
        json!({}),
    )
    .await
}

#[tauri::command]
pub async fn marketing_creative_adopt_music_artifact(
    job_id: String,
    artifact_id: String,
) -> Result<Value, String> {
    marketing_post_json(
        &format!(
            "/api/creative/music/jobs/{}/adopt",
            urlencoding::encode(&job_id)
        ),
        json!({ "artifactId": artifact_id }),
    )
    .await
}

#[tauri::command]
pub async fn marketing_creative_music_feedback(
    artifact_id: String,
    rating: u8,
    reason_tags: Vec<String>,
    comment: Option<String>,
    submit_to_knowledge: bool,
) -> Result<Value, String> {
    marketing_post_json(
        "/api/creative/music/feedback",
        json!({
            "artifactId": artifact_id,
            "rating": rating.clamp(1, 5),
            "reasonTags": reason_tags,
            "comment": comment.unwrap_or_default(),
            "submitToKnowledge": submit_to_knowledge,
        }),
    )
    .await
}

#[tauri::command]
pub async fn marketing_creative_admin_upsert_music_brand(request: Value) -> Result<Value, String> {
    marketing_post_json("/api/admin/creative/music/brands", request).await
}

#[tauri::command]
pub async fn marketing_creative_admin_upsert_music_business_line(
    request: Value,
) -> Result<Value, String> {
    marketing_post_json("/api/admin/creative/music/business-lines", request).await
}

#[tauri::command]
pub async fn marketing_creative_upload_reference(
    request: MarketingCreativeReferenceUploadRequest,
) -> Result<Value, String> {
    let bytes = BASE64_STANDARD
        .decode(request.data_base64.as_bytes())
        .map_err(|e| format!("Invalid reference image data: {e}"))?;
    if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
        return Err("Reference image must be between 1 byte and 20MB".to_string());
    }
    let part = Part::bytes(bytes)
        .file_name(safe_file_name(&request.file_name))
        .mime_str(&request.media_type)
        .map_err(|e| format!("Invalid reference image media type: {e}"))?;
    let form = Form::new().part("referenceImage", part);
    let client = marketing_client()?;
    let url = format!("{}/api/creative/reference-images", marketing_base_url());
    let req = apply_cookie(client.post(url).multipart(form)).await;
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Reference image upload failed: {e}"))?;
    store_response_cookie(&resp).await;
    json_or_error(resp, "Reference image upload").await
}

async fn marketing_transfer_ticket() -> Result<Value, String> {
    let client = marketing_client()?;
    let request = apply_cookie(client.get(format!(
        "{}/api/assets/transfer",
        marketing_base_url()
    )))
    .await;
    let response = request
        .send()
        .await
        .map_err(|e| format!("Marketing transfer authorization failed: {e}"))?;
    json_or_error(response, "Marketing transfer authorization").await
}

fn direct_transfer_url(ticket: &Value, path: &str, scope: &str) -> Result<reqwest::Url, String> {
    let origin = ticket["origin"]
        .as_str()
        .ok_or("Marketing transfer origin is missing")?;
    let credential = ticket[scope]
        .as_str()
        .ok_or_else(|| format!("Marketing {scope} is missing"))?;
    let mut url = reqwest::Url::parse(origin)
        .map_err(|e| format!("Invalid Marketing transfer origin: {e}"))?;
    if url.scheme() != "https"
        && !(cfg!(debug_assertions) && url.host_str() == Some("127.0.0.1"))
    {
        return Err("Marketing transfer origin must use HTTPS".to_string());
    }
    url.set_path(path);
    url.set_query(None);
    url.query_pairs_mut().append_pair("ticket", credential);
    Ok(url)
}

fn direct_asset_url(ticket: &Value, asset_id: &str) -> Result<reqwest::Url, String> {
    direct_transfer_url(
        ticket,
        &format!("/api/assets/direct/{}/file", urlencoding::encode(asset_id)),
        "readTicket",
    )
}

#[tauri::command]
pub async fn marketing_upload_assets(file_paths: Vec<String>, folder: Option<String>) -> Result<Value, String> {
    if file_paths.is_empty() || file_paths.len() > 8 {
        return Err("请选择 1 到 8 个文件上传".to_string());
    }
    let transfer = marketing_transfer_ticket().await?;
    let url = direct_transfer_url(&transfer, "/api/assets/direct/upload", "uploadTicket")?;
    let mut form = Form::new().text("folder", folder.unwrap_or_default());
    for path in file_paths {
        let file = Path::new(&path);
        let metadata = tokio::fs::metadata(file)
            .await
            .map_err(|e| format!("无法读取文件 {}：{e}", file.display()))?;
        if !metadata.is_file() || metadata.len() > 25 * 1024 * 1024 {
            return Err(format!("文件必须小于 25 MB：{}", file.display()));
        }
        let name = file.file_name().and_then(|name| name.to_str())
            .ok_or("上传文件名无效")?;
        let bytes = tokio::fs::read(file)
            .await
            .map_err(|e| format!("无法读取文件 {}：{e}", file.display()))?;
        form = form.part("files", Part::bytes(bytes).file_name(name.to_string()));
    }
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.post(url).multipart(form).send().await
        .map_err(|e| format!("素材上传失败：{e}"))?;
    json_or_error(response, "Marketing asset upload").await
}

async fn download_marketing_asset(
    asset_id: &str,
    requested_file_name: Option<&str>,
) -> Result<DownloadedMarketingAsset, String> {
    let ticket = marketing_transfer_ticket().await?;
    let source_url = direct_asset_url(&ticket, asset_id)?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .user_agent("Harmony-Work/1.0.8 Marketing")
        .build()
        .map_err(|e| e.to_string())?;
    let req = client.get(source_url.clone());
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Marketing asset download failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Marketing asset download returned HTTP {status}: {body}"
        ));
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
        source_url: source_url.to_string(),
        bytes,
    })
}

fn save_target_path(target_path: &str) -> Result<PathBuf, String> {
    let target_path = target_path.trim();
    if target_path.is_empty() {
        return Err("保存路径不能为空".to_string());
    }
    let path = PathBuf::from(target_path);
    if !path.is_absolute() {
        return Err("保存路径必须是绝对路径".to_string());
    }
    if path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .is_none()
    {
        return Err("保存路径必须包含文件名".to_string());
    }
    Ok(path)
}

async fn save_marketing_asset_to_path(
    asset_id: &str,
    requested_file_name: Option<&str>,
    target_path: PathBuf,
) -> Result<MarketingPreparedAsset, String> {
    let asset = download_marketing_asset(asset_id, requested_file_name).await?;
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("无法创建保存目录：{e}"))?;
    }
    tokio::fs::write(&target_path, &asset.bytes)
        .await
        .map_err(|e| format!("无法保存创作成品：{e}"))?;
    Ok(MarketingPreparedAsset {
        asset_id: asset_id.to_string(),
        file_name: asset.file_name,
        media_type: asset.media_type,
        local_path: target_path.to_string_lossy().to_string(),
        source_url: asset.source_url,
        byte_size: asset.bytes.len() as u64,
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
pub async fn marketing_save_asset(
    request: MarketingSaveAssetRequest,
) -> Result<MarketingPreparedAsset, String> {
    let target_path = save_target_path(&request.target_path)?;
    save_marketing_asset_to_path(&request.asset_id, request.file_name.as_deref(), target_path).await
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

#[tauri::command]
pub async fn marketing_creative_save_to_workspace(
    job_id: String,
    workspace_root: Option<String>,
) -> Result<Value, String> {
    let staged = marketing_creative_stage_for_chat(job_id.clone()).await?;
    let asset_ids = staged
        .get("assetIds")
        .and_then(Value::as_array)
        .ok_or_else(|| "创作任务没有可保存的成品".to_string())?;
    if asset_ids.is_empty() {
        return Err("创作任务没有可保存的成品".to_string());
    }
    let job = marketing_creative_job(job_id).await?;
    let artifacts = job
        .get("job")
        .and_then(|value| value.get("artifacts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut saved = Vec::new();
    for asset_id in asset_ids.iter().filter_map(Value::as_str) {
        let file_name = artifacts
            .iter()
            .find(|artifact| artifact.get("assetId").and_then(Value::as_str) == Some(asset_id))
            .and_then(|artifact| artifact.get("fileName"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let saved_asset = marketing_prepare_asset_for_chat(MarketingPrepareAssetRequest {
            asset_id: asset_id.to_string(),
            file_name,
            workspace_root: workspace_root.clone(),
        })
        .await?;
        saved.push(saved_asset);
    }
    Ok(json!({ "job": staged.get("job").cloned(), "saved": saved }))
}

#[cfg(test)]
mod tests {
    use super::{
        build_account_overview, direct_asset_url, direct_transfer_url, normalize_account_balance, normalize_account_gateway_key,
        normalize_account_usage, omit_null_object_fields, save_target_path,
    };
    use serde_json::{json, Value};

    #[test]
    fn direct_asset_url_keeps_the_scope_and_escapes_the_asset_id() {
        let ticket = json!({
            "origin": "https://media.harmonywatch.cn",
            "readTicket": "read-only",
            "uploadTicket": "upload-only"
        });
        let url = direct_asset_url(&ticket, "a/b").unwrap();
        assert_eq!(url.path(), "/api/assets/direct/a%2Fb/file");
        assert_eq!(url.query_pairs().find(|(key, _)| key == "ticket").unwrap().1, "read-only");
        let upload = direct_transfer_url(&ticket, "/api/assets/direct/upload", "uploadTicket").unwrap();
        assert_eq!(upload.query_pairs().find(|(key, _)| key == "ticket").unwrap().1, "upload-only");
    }

    #[test]
    fn direct_transfer_rejects_plain_http() {
        let ticket = json!({"origin": "http://media.harmonywatch.cn", "readTicket": "secret"});
        assert!(direct_asset_url(&ticket, "asset").is_err());
    }

    #[test]
    fn direct_asset_url_preserves_nonstandard_https_port() {
        let ticket = json!({
            "origin": "https://media.harmonywatch.cn:8443",
            "readTicket": "read-only"
        });
        let url = direct_asset_url(&ticket, "asset").unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("media.harmonywatch.cn"));
        assert_eq!(url.port(), Some(8443));
    }

    #[test]
    fn accepts_an_absolute_user_selected_save_target() {
        #[cfg(target_os = "windows")]
        let expected = r"C:\Users\Admin\Desktop\Harvis 成品.mp3";
        #[cfg(not(target_os = "windows"))]
        let expected = "/tmp/Harvis 成品.mp3";
        let path = save_target_path(expected).unwrap();
        assert_eq!(path.to_string_lossy(), expected);
    }

    #[test]
    fn rejects_a_relative_save_target() {
        assert!(save_target_path("Harvis 成品.mp3").is_err());
    }

    #[test]
    fn omits_null_optional_fields_from_marketing_payloads() {
        let payload = omit_null_object_fields(json!({
            "businessLine": "腕表如诗",
            "model": null,
            "theme": "节日礼赠"
        }));
        assert_eq!(payload.get("model"), None);
        assert_eq!(
            payload.get("theme").and_then(Value::as_str),
            Some("节日礼赠")
        );
    }

    #[test]
    fn normalizes_account_balance_to_number() {
        assert_eq!(
            normalize_account_balance(&json!({"balance": 12.5, "quota": 6250000})),
            Some(12.5)
        );
        assert_eq!(
            normalize_account_balance(&json!({"balance": null, "message": "尚未开通中转站账号"})),
            None
        );
    }

    #[test]
    fn normalizes_account_usage_and_drops_absent_stat() {
        let stat = normalize_account_usage(&json!({
            "stat": {"quota": 100, "request_count": 3},
            "logs": []
        }));
        assert_eq!(stat, Some(json!({"quota": 100, "request_count": 3})));

        let absent = normalize_account_usage(&json!({
            "stat": null,
            "logs": [],
            "message": "尚未开通中转站账号"
        }));
        assert_eq!(absent, None);
    }

    #[test]
    fn normalizes_account_gateway_key_and_ignores_null() {
        assert_eq!(
            normalize_account_gateway_key(&json!({
                "gatewayUserId": 1,
                "gatewayApiKey": "sk-test",
                "baseUrl": "https://api.harmonywatch.cn:13001"
            }))
            .as_deref(),
            Some("sk-test")
        );
        assert_eq!(
            normalize_account_gateway_key(&json!({"gatewayApiKey": null})),
            None
        );
    }

    #[test]
    fn builds_account_overview_with_partial_failures() {
        let overview = build_account_overview(
            json!({"id": "u1", "name": "张三", "email": "a@b.c", "userRole": "admin", "approvalStatus": "active"}),
            Some(json!({"balance": 8.0})),
            None,
            Some(json!({"gatewayApiKey": "sk-123"})),
        );

        assert_eq!(overview.balance, Some(8.0));
        assert_eq!(overview.usage, None);
        assert_eq!(overview.gateway_api_key.as_deref(), Some("sk-123"));
        assert_eq!(overview.user.get("name").and_then(Value::as_str), Some("张三"));

        // 序列化后的账户概览可被前端直接读取，且 key 只存在于内存返回值中。
        let serialized = serde_json::to_value(&overview).unwrap();
        assert_eq!(serialized.get("gatewayApiKey").and_then(Value::as_str), Some("sk-123"));
        assert_eq!(serialized.get("usage"), Some(&Value::Null));
    }
}
