use crate::HostContext;
use anyhow::{anyhow, Context, Result};
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
            "description":"Generate a new image, poster, invitation, campaign visual, or image-to-image variation with Harvis GPT Image2. Text-to-image requires only a prompt; reference_ids are optional and only enable image-to-image.",
            "inputSchema":{
                "type":"object","properties":{
                    "prompt":{"type":"string","minLength":1},
                    "canvas_preset":{"type":"string","enum":["television","vertical","store-poster","phone-fullscreen","custom"],"default":"vertical"},
                    "output_width":{"type":"integer","minimum":256,"maximum":4096},
                    "output_height":{"type":"integer","minimum":256,"maximum":4096},
                    "reference_ids":{"type":"array","items":{"type":"string"}}
                },"required":["prompt"],"additionalProperties":false
            },
            "annotations":{"title":"Harvis 图片生成","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}
        }),
        json!({
            "name":"harvis_music_generate",
            "description":"Create instrumental music or BGM with Harvis Suno V5.5 from a finished music-generation prompt.",
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
            "description":"Read a Harvis creative job's progress and completed result metadata. Omit job_id to list recent jobs.",
            "inputSchema":{"type":"object","properties":{"job_id":{"type":"string"}},"additionalProperties":false},
            "annotations":{"title":"查询 Harvis 任务","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
        json!({
            "name":"harvis_save_result",
            "description":"Save completed Harvis image or music results into the active workspace, or archive them into the material library. Use workspace to make files available to the current conversation.",
            "inputSchema":{
                "type":"object","properties":{
                    "job_id":{"type":"string","minLength":1},
                    "destination":{"type":"string","enum":["workspace","material_library"]},
                    "workspace_root":{"type":"string","description":"Required for workspace destination; absolute active workspace path."}
                },"required":["job_id","destination"],"additionalProperties":false
            },
            "annotations":{"title":"保存 Harvis 成品","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
    ]
}

pub(crate) async fn call(context: &HostContext, name: &str, arguments: &Value) -> Result<Value> {
    let client = HarvisClient::new(context)?;
    match name {
        "harvis_image_generate" => {
            let request = normalize_image_request(arguments)?;
            mcp_success(
                client
                    .post_json("/api/creative/image/jobs", request)
                    .await?,
            )
        }
        "harvis_music_generate" => {
            let request = normalize_music_request(arguments)?;
            mcp_success(
                client
                    .post_json("/api/creative/music/jobs", request)
                    .await?,
            )
        }
        "harvis_job_status" => {
            let payload = if let Some(id) = optional_string(arguments, "job_id") {
                client
                    .get_json(&format!("/api/creative/jobs/{}", urlencoding::encode(&id)))
                    .await?
            } else {
                client.get_json("/api/creative/jobs").await?
            };
            mcp_success(payload)
        }
        "harvis_save_result" => save_result(&client, arguments).await,
        _ => Err(anyhow!("unknown Harvis tool: {name}")),
    }
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
                .user_agent("Xiaoheng-WorkBuddy-MCP/1.0")
                .build()?,
        })
    }

    async fn request(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let request = match tokio::fs::read_to_string(self.context.marketing_cookie_path()).await {
            Ok(cookie) if !cookie.trim().is_empty() => request.header(COOKIE, cookie.trim()),
            _ => request,
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
            tokio::fs::write(path, cookies.join("; ")).await?;
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
    let references = input
        .get("reference_ids")
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
        ("referenceIds", references.map(Value::Array)),
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

    #[test]
    fn text_to_image_does_not_require_references() {
        let request = normalize_image_request(&json!({"prompt":"七夕腕表海报"})).unwrap();
        assert_eq!(request["mode"], "text");
        assert_eq!(request["canvasPreset"], "vertical");
        assert!(request.get("referenceIds").is_none());
    }
}
