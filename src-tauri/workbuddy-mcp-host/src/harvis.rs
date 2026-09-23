use crate::{harvis_responses, service_urls::HARMONY_API_BASE, HostContext};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::Instant;

const HARVIS_TIMEOUT_SECS: u64 = 180;

pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "harvis_image_generate"
            | "harvis_music_generate"
            | "harvis_job_status"
            | "harvis_save_result"
    )
}

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"harvis_image_generate",
            "description":"Submit an image task through the user's Harmony API key and return its resumable task ID immediately. Poll with harvis_job_status. Text-to-image requires only a prompt; reference_urls enable image-to-image.",
            "inputSchema":{
                "type":"object","properties":{
                    "prompt":{"type":"string","minLength":1},
                    "canvas_preset":{"type":"string","enum":["television","vertical","store-poster","phone-fullscreen","custom"],"default":"vertical"},
                    "output_width":{"type":"integer","minimum":256,"maximum":4096},
                    "output_height":{"type":"integer","minimum":256,"maximum":4096},
                    "reference_urls":{"type":"array","items":{"type":"string"}},
                    "reference_ids":{"type":"array","items":{"type":"string"},"description":"Legacy portal reference IDs are not usable with Harmony Responses; provide public reference_urls instead."}
                },"required":["prompt"],"additionalProperties":false
            },
            "annotations":{"title":"Harvis 图片生成","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}
        }),
        json!({
            "name":"harvis_music_generate",
            "description":"Submit instrumental music or BGM through the user's Harmony API key and return its resumable task ID immediately. Poll with harvis_job_status.",
            "inputSchema":{
                "type":"object","properties":{
                    "prompt":{"type":"string","minLength":1},"title":{"type":"string"},
                    "duration":{"type":"integer","minimum":10,"maximum":600,"default":45},
                    "mode":{"type":"string","enum":["free","assistant"],"default":"free"}
                },"required":["prompt"],"additionalProperties":false
            },
            "annotations":{"title":"Harvis 音乐生成","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}
        }),
        json!({
            "name":"harvis_job_status",
            "description":"Wait for a Harvis creative task and return progress or completed media. Re-call with the same job_id while processing. Omit job_id to list recent local submissions.",
            "inputSchema":{"type":"object","properties":{"job_id":{"type":"string"},"kind":{"type":"string","enum":["image","music","video"]}},"additionalProperties":false},
            "annotations":{"title":"查询 Harvis 任务","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
        json!({
            "name":"harvis_save_result",
            "description":"Save completed Harvis image or music results into the active workspace. Material-library archiving is supported only for legacy portal jobs. Use workspace to make files available to the current conversation.",
            "inputSchema":{
                "type":"object","properties":{
                    "job_id":{"type":"string","minLength":1},
                    "destination":{"type":"string","enum":["workspace","material_library"]},
                    "workspace_root":{"type":"string","description":"Required for workspace destination; absolute active workspace path."},
                    "kind":{"type":"string","enum":["image","music","video"]}
                },"required":["job_id","destination"],"additionalProperties":false
            },
            "annotations":{"title":"保存 Harvis 成品","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
    ]
}

pub(crate) async fn call(context: &HostContext, name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "harvis_image_generate" => {
            let request = normalize_image_request(arguments)?;
            mcp_success(
                HarmonyClient::new(context)?
                    .submit("image", &request)
                    .await?,
            )
        }
        "harvis_music_generate" => {
            let request = normalize_music_request(arguments)?;
            mcp_success(
                HarmonyClient::new(context)?
                    .submit("music", &request)
                    .await?,
            )
        }
        "harvis_job_status" => {
            let payload = if let Some(id) = optional_string(arguments, "job_id") {
                if id.starts_with("resp_") {
                    let kind = optional_string(arguments, "kind")
                        .or_else(|| job_kind(context, &id))
                        .ok_or_else(|| anyhow!("请提供任务类型 kind（image 或 music）"))?;
                    HarmonyClient::new(context)?
                        .wait(&id, &kind, Duration::from_secs(20))
                        .await?
                } else {
                    HarvisClient::new(context)?
                        .get_json(&format!("/api/creative/jobs/{}", urlencoding::encode(&id)))
                        .await?
                }
            } else {
                json!({"jobs":read_jobs(context)?,"source":"local_responses"})
            };
            mcp_success(payload)
        }
        "harvis_save_result" => {
            let id = required_string(arguments, "job_id")?;
            if id.starts_with("resp_") {
                save_harmony_result(context, arguments).await
            } else {
                save_result(&HarvisClient::new(context)?, arguments).await
            }
        }
        _ => Err(anyhow!("unknown Harvis tool: {name}")),
    }
}

struct HarmonyClient<'a> {
    context: &'a HostContext,
    client: reqwest::Client,
    key: String,
    settings: Value,
    base_url: String,
}

impl<'a> HarmonyClient<'a> {
    fn new(context: &'a HostContext) -> Result<Self> {
        let settings: Value = serde_json::from_str(
            &std::fs::read_to_string(context.app_data_dir.join("config.json"))
                .context("无法读取 Harmony 配置，请先在客户端完成设置")?,
        )?;
        let encrypted = required_string(&settings, "harmony_api_key")
            .map_err(|_| anyhow!("请先在设置中配置 Harmony API Key"))?;
        let store = crate::secrets::SecretStore::new(&context.app_data_dir)?;
        let key = store.decrypt_hex(&encrypted).unwrap_or(encrypted);
        Ok(Self {
            context,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(30))
                .user_agent("Harmony-Work-MCP/1.0")
                .build()?,
            key,
            settings,
            base_url: HARMONY_API_BASE.to_string(),
        })
    }

    fn model(&self, kind: &str) -> Result<&str> {
        let field = match kind {
            "image" => "harvis_image_model",
            "music" => "harvis_music_model",
            "video" => "harvis_video_model",
            _ => return Err(anyhow!("不支持的 Harvis 任务类型：{kind}")),
        };
        self.settings
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("请先在设置中选择 Harvis {kind} 模型"))
    }

    async fn response_json(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request
            .bearer_auth(&self.key)
            .send()
            .await
            .context("Harmony 创作请求失败")?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(anyhow!("Harmony 创作请求失败（{status}）：{body}"));
        }
        serde_json::from_str(&body).context("Harmony 创作响应不是有效 JSON")
    }

    async fn submit(&self, kind: &str, request: &Value) -> Result<Value> {
        let body = harvis_responses::generation_body(self.model(kind)?, kind, request)
            .map_err(|error| anyhow!(error))?;
        let response = self
            .response_json(
                self.client
                    .post(format!("{}/responses", self.base_url))
                    .json(&body),
            )
            .await?;
        let id =
            required_string(&response, "id").context("Harmony 未返回任务 ID，无法跟踪创作进度")?;
        record_job(self.context, &id, kind)?;
        Ok(harvis_responses::task_result(&response, kind, &id))
    }

    async fn wait(&self, id: &str, kind: &str, max_wait: Duration) -> Result<Value> {
        let started = Instant::now();
        loop {
            let response = self
                .response_json(self.client.get(format!(
                    "{}/responses/{}",
                    self.base_url,
                    urlencoding::encode(id)
                )))
                .await?;
            let result = harvis_responses::task_result(&response, kind, id);
            if is_terminal(&result) || started.elapsed() >= max_wait {
                return Ok(result);
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}

fn is_terminal(result: &Value) -> bool {
    matches!(
        result.get("status").and_then(Value::as_str),
        Some("succeeded" | "failed")
    )
}

fn jobs_path(context: &HostContext) -> PathBuf {
    context.app_data_dir.join("harvis-mcp-jobs.json")
}

fn read_jobs(context: &HostContext) -> Result<Vec<Value>> {
    match std::fs::read_to_string(jobs_path(context)) {
        Ok(content) => Ok(serde_json::from_str(&content).context("Harvis 本地任务记录无法解析")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn record_job(context: &HostContext, id: &str, kind: &str) -> Result<()> {
    let mut jobs = read_jobs(context)?;
    jobs.retain(|job| job.get("job_id").and_then(Value::as_str) != Some(id));
    jobs.insert(
        0,
        json!({"job_id":id,"kind":kind,"created_at":chrono::Utc::now().to_rfc3339()}),
    );
    jobs.truncate(100);
    std::fs::create_dir_all(&context.app_data_dir)?;
    std::fs::write(jobs_path(context), serde_json::to_vec(&jobs)?)?;
    Ok(())
}

fn job_kind(context: &HostContext, id: &str) -> Option<String> {
    read_jobs(context)
        .ok()?
        .into_iter()
        .find(|job| job.get("job_id").and_then(Value::as_str) == Some(id))?
        .get("kind")?
        .as_str()
        .map(str::to_string)
}

struct HarvisClient<'a> {
    context: &'a HostContext,
    client: reqwest::Client,
}

impl<'a> HarvisClient<'a> {
    fn new(context: &'a HostContext) -> Result<Self> {
        Ok(Self {
            context,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(HARVIS_TIMEOUT_SECS))
                .user_agent("Harmony-Work-MCP/1.0")
                .build()?,
        })
    }

    async fn request(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let cookie_path = self.context.marketing_cookie_path();
        let request = match read_session_cookie(&cookie_path)? {
            Some(cookie) => request.header(COOKIE, cookie),
            None => request,
        };
        let response = request
            .send()
            .await
            .context("Harvis gateway request failed")?;
        let cookies = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !cookies.is_empty() {
            let path = self.context.marketing_cookie_path();
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let store = crate::secrets::SecretStore::new(
                path.parent().context("Missing session directory")?,
            )?;
            tokio::fs::write(path, store.encrypt_hex(&cookies.join("; "))?).await?;
        }
        Ok(response)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self
            .request(
                self.client
                    .get(format!("{}{}", self.context.marketing_base_url, path)),
            )
            .await?;
        json_response(response).await
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .request(
                self.client
                    .post(format!("{}{}", self.context.marketing_base_url, path))
                    .json(&body),
            )
            .await?;
        json_response(response).await
    }

    async fn download_asset(
        &self,
        asset_id: &str,
        requested_name: Option<&str>,
    ) -> Result<DownloadedAsset> {
        let source_url = format!(
            "{}/api/assets/{}/file",
            self.context.marketing_base_url,
            urlencoding::encode(asset_id)
        );
        let response = self.request(self.client.get(&source_url)).await?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Harvis asset download returned HTTP {status}: {}",
                response.text().await.unwrap_or_default()
            ));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let disposition_name = response
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_disposition);
        let bytes = response.bytes().await?.to_vec();
        let file_name = safe_file_name(
            requested_name
                .or(disposition_name.as_deref())
                .unwrap_or("harvis-asset"),
        );
        Ok(DownloadedAsset {
            media_type: media_type_for_name(&file_name, content_type.as_deref()),
            file_name,
            source_url,
            bytes,
        })
    }
}

struct DownloadedAsset {
    file_name: String,
    media_type: String,
    source_url: String,
    bytes: Vec<u8>,
}

fn read_session_cookie(path: &Path) -> Result<Option<String>> {
    let value = match std::fs::read_to_string(path) {
        Ok(value) => value.trim().to_string(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if value.is_empty() {
        return Ok(None);
    }
    // A legacy plaintext cookie has a name=value pair; encrypted data is hex.
    if value.contains('=') {
        return Ok(Some(value));
    }
    let directory = path.parent().context("Missing session directory")?;
    if !directory.join(".secret_key").is_file() {
        return Err(anyhow!("登录凭据无法读取，请在客户端重新登录"));
    }
    let cookie = crate::secrets::SecretStore::new(directory)?
        .decrypt_hex(&value)
        .map_err(|_| anyhow!("登录凭据无法解密，请在客户端重新登录"))?;
    Ok(Some(cookie))
}

#[cfg(test)]
mod session_regression_tests {
    use super::*;

    #[test]
    fn desktop_encrypted_session_is_readable_by_mcp() {
        let dir = std::env::temp_dir().join(format!("harvis-session-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::secrets::SecretStore::new(&dir).unwrap();
        let path = dir.join("marketing.session.cookie");
        let expected = "harmony_session=regression-test";
        std::fs::write(&path, store.encrypt_hex(expected).unwrap()).unwrap();
        assert_eq!(
            read_session_cookie(&path).unwrap().as_deref(),
            Some(expected)
        );
        std::fs::write(&path, "invalid-ciphertext").unwrap();
        assert!(read_session_cookie(&path).is_err());
        std::fs::write(&path, expected).unwrap();
        assert_eq!(
            read_session_cookie(&path).unwrap().as_deref(),
            Some(expected)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

async fn json_response(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!("Harvis gateway returned HTTP {status}: {body}"));
    }
    serde_json::from_str(&body)
        .with_context(|| format!("Harvis gateway returned invalid JSON: {body}"))
}

fn normalize_image_request(input: &Value) -> Result<Value> {
    let prompt = required_string(input, "prompt")?;
    let legacy_ids = input
        .get("reference_ids")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty());
    if legacy_ids.is_some() {
        return Err(anyhow!(
            "Harmony 创作需要 reference_urls，旧素材 ID 不能直接作为参考图地址"
        ));
    }
    let references = input
        .get("reference_urls")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .cloned();
    Ok(compact_object([
        (
            "mode",
            Some(Value::String(
                if references.is_some() {
                    "image"
                } else {
                    "text"
                }
                .to_string(),
            )),
        ),
        ("prompt", Some(Value::String(prompt))),
        (
            "canvasPreset",
            Some(Value::String(
                optional_string(input, "canvas_preset").unwrap_or_else(|| "vertical".to_string()),
            )),
        ),
        ("outputWidth", input.get("output_width").cloned()),
        ("outputHeight", input.get("output_height").cloned()),
        ("referenceUrls", references.map(Value::Array)),
    ]))
}

fn normalize_music_request(input: &Value) -> Result<Value> {
    let prompt = required_string(input, "prompt")?;
    Ok(compact_object([
        ("prompt", Some(Value::String(prompt.clone()))),
        ("tags", Some(Value::String(prompt))),
        (
            "title",
            Some(Value::String(
                optional_string(input, "title").unwrap_or_else(|| "Harvis 背景音乐".to_string()),
            )),
        ),
        (
            "duration",
            Some(
                input
                    .get("duration")
                    .cloned()
                    .unwrap_or_else(|| Value::from(45)),
            ),
        ),
        (
            "mode",
            Some(Value::String(
                optional_string(input, "mode").unwrap_or_else(|| "free".to_string()),
            )),
        ),
    ]))
}

async fn save_harmony_result(context: &HostContext, input: &Value) -> Result<Value> {
    let id = required_string(input, "job_id")?;
    let destination = required_string(input, "destination")?;
    if destination != "workspace" {
        return Err(anyhow!(
            "Harmony Responses 成品请先保存到工作区，再通过素材库导入；旧营销任务仍可直接归档"
        ));
    }
    let workspace_root = PathBuf::from(required_string(input, "workspace_root")?);
    if !workspace_root.is_absolute() {
        return Err(anyhow!("workspace_root must be an absolute path"));
    }
    let kind = optional_string(input, "kind")
        .or_else(|| job_kind(context, &id))
        .ok_or_else(|| anyhow!("无法确定任务类型，请提供 kind（image 或 music）"))?;
    let client = HarmonyClient::new(context)?;
    let result = client.wait(&id, &kind, Duration::ZERO).await?;
    if result["status"] != "succeeded" {
        return Err(anyhow!(
            "Harvis 任务尚未完成：{}（{}）",
            result["status"],
            id
        ));
    }
    let result_url = required_string(&result, "result_url")?;
    let (bytes, media_type, extension) =
        download_harmony_media(&client.client, &result_url, &kind).await?;
    let directory = workspace_root.join("marketing-assets");
    tokio::fs::create_dir_all(&directory).await?;
    let file_name = safe_file_name(&format!("Harvis-{id}.{extension}"));
    let path = unique_target(&directory, &file_name);
    tokio::fs::write(&path, &bytes).await?;
    let payload = json!({
        "job_id":id,"kind":kind,"status":"saved",
        "saved":[{"fileName":file_name,"mediaType":media_type,"localPath":path.to_string_lossy(),"sourceUrl":result_url,"byteSize":bytes.len()}]
    });
    Ok(json!({"content":[
        {"type":"text","text":serde_json::to_string_pretty(&payload)?},
        {"type":"resource_link","name":file_name,"uri":file_uri(&path),"mimeType":media_type,"size":bytes.len()}
    ],"structuredContent":payload,"isError":false}))
}

async fn download_harmony_media(
    client: &reqwest::Client,
    result_url: &str,
    kind: &str,
) -> Result<(Vec<u8>, String, &'static str)> {
    const LIMIT: usize = 256 * 1024 * 1024;
    let url = harvis_responses::media_url(result_url).ok_or_else(|| anyhow!("成品地址无效"))?;
    let (bytes, content_type) = if url.starts_with("data:") {
        let (header, encoded) = url.split_once(',').context("成品数据格式无效")?;
        if !header.ends_with(";base64") || encoded.len() > LIMIT * 4 / 3 + 4 {
            return Err(anyhow!("成品超过 256MB 大小限制"));
        }
        (
            base64::engine::general_purpose::STANDARD.decode(encoded)?,
            header
                .trim_start_matches("data:")
                .trim_end_matches(";base64")
                .to_string(),
        )
    } else {
        let mut response = client.get(&url).send().await?.error_for_status()?;
        let mime = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_string();
        if mime == "text/html" || mime == "application/json" {
            return Err(anyhow!("成品链接没有返回媒体文件"));
        }
        if response
            .content_length()
            .is_some_and(|size| size > LIMIT as u64)
        {
            return Err(anyhow!("成品超过 256MB 大小限制"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > LIMIT {
                return Err(anyhow!("成品超过 256MB 大小限制"));
            }
            bytes.extend_from_slice(&chunk);
        }
        (bytes, mime)
    };
    if bytes.is_empty() {
        return Err(anyhow!("成品内容为空"));
    }
    let expected_prefix = match kind {
        "image" => "image/",
        "music" => "audio/",
        "video" => "video/",
        _ => "",
    };
    if !content_type.is_empty()
        && !content_type.starts_with(expected_prefix)
        && content_type != "application/octet-stream"
    {
        return Err(anyhow!("成品链接返回了不匹配的媒体类型：{content_type}"));
    }
    let (default_mime, default_extension) = match kind {
        "image" => ("image/png", "png"),
        "music" => ("audio/mpeg", "mp3"),
        "video" => ("video/mp4", "mp4"),
        _ => return Err(anyhow!("不支持的成品类型")),
    };
    let extension = match content_type.as_str() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/png" => "png",
        "audio/wav" => "wav",
        "audio/mp4" => "m4a",
        "audio/mpeg" => "mp3",
        "video/mp4" => "mp4",
        _ => default_extension,
    };
    Ok((
        bytes,
        if content_type.is_empty() {
            default_mime.to_string()
        } else {
            content_type
        },
        extension,
    ))
}

async fn save_result(client: &HarvisClient<'_>, input: &Value) -> Result<Value> {
    let job_id = required_string(input, "job_id")?;
    let destination = required_string(input, "destination")?;
    let staged = client
        .post_json(
            &format!(
                "/api/creative/jobs/{}/stage-for-chat",
                urlencoding::encode(&job_id)
            ),
            json!({}),
        )
        .await?;
    if destination == "material_library" {
        return mcp_success(staged);
    }
    if destination != "workspace" {
        return Err(anyhow!("destination must be workspace or material_library"));
    }
    let workspace_root = PathBuf::from(required_string(input, "workspace_root")?);
    if !workspace_root.is_absolute() {
        return Err(anyhow!("workspace_root must be an absolute path"));
    }
    let asset_ids = staged
        .get("assetIds")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Harvis job has no completed artifacts"))?;
    if asset_ids.is_empty() {
        return Err(anyhow!("Harvis job has no completed artifacts"));
    }
    let job = client
        .get_json(&format!(
            "/api/creative/jobs/{}",
            urlencoding::encode(&job_id)
        ))
        .await?;
    let artifacts = job
        .pointer("/job/artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let directory = workspace_root.join("marketing-assets");
    tokio::fs::create_dir_all(&directory).await?;
    let mut saved = Vec::new();
    let mut content = Vec::new();
    for asset_id in asset_ids.iter().filter_map(Value::as_str) {
        let requested_name = artifacts
            .iter()
            .find(|artifact| artifact.get("assetId").and_then(Value::as_str) == Some(asset_id))
            .and_then(|artifact| artifact.get("fileName"))
            .and_then(Value::as_str);
        let asset = client.download_asset(asset_id, requested_name).await?;
        let path = unique_target(&directory, &asset.file_name);
        tokio::fs::write(&path, &asset.bytes).await?;
        let path_text = path.to_string_lossy().to_string();
        saved.push(json!({
            "assetId":asset_id,"fileName":asset.file_name,"mediaType":asset.media_type,
            "localPath":path_text,"sourceUrl":asset.source_url,"byteSize":asset.bytes.len()
        }));
        content.push(json!({"type":"resource_link","name":asset.file_name,"uri":file_uri(&path),"mimeType":asset.media_type,"size":asset.bytes.len()}));
    }
    let payload = json!({"job":staged.get("job").cloned(),"saved":saved,"artifacts":saved});
    content.insert(
        0,
        json!({"type":"text","text":serde_json::to_string_pretty(&payload)?}),
    );
    Ok(json!({"content":content,"structuredContent":payload,"isError":false}))
}

fn mcp_success(payload: Value) -> Result<Value> {
    Ok(json!({
        "content":[{"type":"text","text":serde_json::to_string_pretty(&payload)?}],
        "structuredContent":payload,"isError":false
    }))
}

fn compact_object(entries: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
            .collect::<Map<_, _>>(),
    )
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    optional_string(value, key).ok_or_else(|| anyhow!("{key} is required"))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn safe_file_name(name: &str) -> String {
    let cleaned = name
        .trim()
        .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    if cleaned.is_empty() {
        "harvis-asset".to_string()
    } else {
        cleaned
    }
}

fn filename_from_disposition(value: &str) -> Option<String> {
    value.split(';').map(str::trim).find_map(|part| {
        part.strip_prefix("filename=")
            .map(|name| name.trim_matches('"').to_string())
    })
}

fn media_type_for_name(name: &str, fallback: Option<&str>) -> String {
    if let Some(value) = fallback.filter(|value| !value.trim().is_empty()) {
        return value.to_string();
    }
    match Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn unique_target(directory: &Path, file_name: &str) -> PathBuf {
    let initial = directory.join(file_name);
    if !initial.exists() {
        return initial;
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("harvis-asset");
    let extension = path.extension().and_then(|v| v.to_str());
    for index in 2..10_000 {
        let name = match extension {
            Some(ext) => format!("{stem}-{index}.{ext}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = directory.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!(
        "{}-{}",
        stem,
        chrono::Utc::now().timestamp_millis()
    ))
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn text_to_image_does_not_require_references() {
        let request = normalize_image_request(&json!({"prompt":"七夕腕表海报"})).unwrap();
        assert_eq!(request["mode"], "text");
        assert_eq!(request["canvasPreset"], "vertical");
        assert!(request.get("referenceUrls").is_none());
    }

    #[tokio::test]
    async fn submits_with_personal_key_and_returns_a_resumable_id_without_polling() {
        let root =
            std::env::temp_dir().join(format!("harvis-mcp-responses-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let secret = crate::secrets::SecretStore::new(&root).unwrap();
        let key = secret.encrypt_hex("personal-harmony-key").unwrap();
        std::fs::write(
            root.join("config.json"),
            serde_json::to_vec(&json!({
                "harmony_api_key":key,"harvis_image_model":"gpt-image2-test"
            }))
            .unwrap(),
        )
        .unwrap();
        let context = HostContext::new(
            root.clone(),
            root.join("workbuddy.db"),
            None,
            vec![],
            String::new(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                let headers_end = request.windows(4).position(|part| part == b"\r\n\r\n");
                if let Some(end) = headers_end {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("POST /v1/responses HTTP/1.1"));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer personal-harmony-key"));
            assert!(request.contains("\"model\":\"gpt-image2-test\""));
            assert!(request.contains("\"background\":true"));
            let body = r#"{"id":"resp_test","status":"queued"}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let mut client = HarmonyClient::new(&context).unwrap();
        client.base_url = format!("http://{address}/v1");
        let request = normalize_image_request(&json!({"prompt":"腕表海报"})).unwrap();
        let result = client.submit("image", &request).await.unwrap();
        assert_eq!(result["status"], "pending");
        assert_eq!(result["task_id"], "resp_test");
        assert_eq!(result["job_id"], "resp_test");
        assert_eq!(job_kind(&context, "resp_test").as_deref(), Some("image"));
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
