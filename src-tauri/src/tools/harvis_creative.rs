use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{
    Tool, ToolContext, ToolExecutionMode, ToolExposure, ToolMetadata, ToolResult, ToolRisk,
};
use piscis_kernel::tools::output::ToolArtifact;
use serde_json::{json, Map, Value};
use std::borrow::Cow;

pub struct HarvisCreativeTool;
pub struct HarvisImageGenerateTool;
pub struct HarvisMusicGenerateTool;
pub struct HarvisJobStatusTool;
pub struct HarvisSaveResultTool;

fn metadata(
    risk: ToolRisk,
    execution: ToolExecutionMode,
    produces_artifacts: bool,
) -> ToolMetadata {
    ToolMetadata {
        category: "harvis_creative",
        exposure: ToolExposure::Deferred,
        execution,
        risk,
        timeout_secs: if risk == ToolRisk::External { 180 } else { 120 },
        produces_artifacts,
    }
}

fn payload_result(payload: Value) -> ToolResult {
    let content = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    ToolResult::ok(content).with_data(payload)
}

fn saved_artifacts(payload: &Value) -> Vec<ToolArtifact> {
    payload
        .get("saved")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|saved| {
            let name = saved.get("fileName")?.as_str()?.to_string();
            let media_type = saved
                .get("mediaType")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            let artifact_type = if media_type.starts_with("image/") {
                "image"
            } else if media_type.starts_with("audio/") {
                "audio"
            } else if media_type.starts_with("video/") {
                "video"
            } else {
                "file"
            };
            Some(ToolArtifact {
                name,
                artifact_type: artifact_type.to_string(),
                uri: saved
                    .get("localPath")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                content_summary: "Harvis 创作成品已保存到当前工作区".to_string(),
                metadata: Some(saved.clone()),
            })
        })
        .collect()
}

fn compact_object(entries: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_string(), value)))
            .collect(),
    )
}

fn request_object(input: &Value) -> Result<Value> {
    input
        .get("request")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("request must be an object"))
}

fn job_id(input: &Value) -> Result<String> {
    input
        .get("job_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("job_id is required"))
}

fn required_text(request: &Map<String, Value>, key: &str) -> Result<String> {
    request
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("{key} is required"))
}

/// Keep agent calls compatible with both the current UI API and the earlier
/// documented shape. This is intentionally forgiving: the model should be able
/// to make a single creative call without inventing a reference-image step.
fn normalize_music_request(input: &Value) -> Result<Value> {
    let mut request = request_object(input)?;
    let object = request
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("request must be an object"))?;
    let tags = object
        .get("tags")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            object
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("create_music needs a non-empty prompt or tags"))?;
    object
        .entry("mode")
        .or_insert_with(|| Value::String("free".to_string()));
    object
        .entry("title")
        .or_insert_with(|| Value::String("Harvis 背景音乐".to_string()));
    object.entry("duration").or_insert_with(|| Value::from(45));
    object.insert("tags".to_string(), Value::String(tags.clone()));
    object
        .entry("prompt")
        .or_insert_with(|| Value::String(tags));
    Ok(request)
}

fn normalize_image_request(input: &Value) -> Result<Value> {
    let mut request = request_object(input)?;
    let object = request
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("request must be an object"))?;
    let _ = required_text(object, "prompt")?;

    if !object.contains_key("canvasPreset") {
        let legacy_canvas = object.get("canvas").cloned();
        if let Some(canvas) = legacy_canvas.as_ref().and_then(Value::as_object) {
            if let Some(preset) = canvas.get("preset").and_then(Value::as_str) {
                object.insert(
                    "canvasPreset".to_string(),
                    Value::String(preset.to_string()),
                );
            }
            if let Some(width) = canvas.get("width") {
                object.entry("outputWidth").or_insert_with(|| width.clone());
            }
            if let Some(height) = canvas.get("height") {
                object
                    .entry("outputHeight")
                    .or_insert_with(|| height.clone());
            }
        }
    }
    object
        .entry("canvasPreset")
        .or_insert_with(|| Value::String("vertical".to_string()));
    let mode = object
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .trim()
        .to_ascii_lowercase();
    object.insert(
        "mode".to_string(),
        Value::String(
            if matches!(mode.as_str(), "image" | "reference" | "img2img") {
                "image".to_string()
            } else {
                "text".to_string()
            },
        ),
    );
    Ok(request)
}

#[async_trait]
impl Tool for HarvisCreativeTool {
    fn name(&self) -> &str {
        "harvis_creative"
    }

    fn description(&self) -> &str {
        "Create Harvis background music or images through the authenticated Harmony gateway. Text-to-image is supported directly and never requires a reference image; image-to-image accepts existing referenceIds. Use only when the user explicitly asks to generate music, BGM, an image, a poster, or another visual asset."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Generate Harvis Suno music or GPT Image2 images, query progress, save completed results into the current workspace, or archive them into the user's material library.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create_music", "create_image", "get_job", "list_jobs", "stage_for_chat", "save_to_workspace"],
                    "description": "create_music submits Suno V5.5; create_image submits GPT Image2; get_job and list_jobs read status; stage_for_chat archives completed results; save_to_workspace downloads all completed artifacts to the current session workspace."
                },
                "request": {
                    "type": "object",
                    "description": "For create_music: {prompt or tags, title?: string, duration?: 45, mode?: 'free'|'assistant'}. For direct text-to-image: {mode:'text', prompt:'...', canvasPreset:'vertical'|'television'|'store-poster'|'phone-fullscreen'|'custom', outputWidth?: number, outputHeight?: number}. Do not require referenceIds for text-to-image. For image-to-image only: {mode:'image', prompt:'...', canvasPreset:'vertical', referenceIds:['uploaded-reference-id']}."
                },
                "job_id": { "type": "string", "description": "Creative job id for get_job, stage_for_chat, or save_to_workspace." }
            }
        })
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            exposure: ToolExposure::Hidden,
            ..metadata(ToolRisk::External, ToolExecutionMode::Exclusive, true)
        }
    }

    fn needs_confirmation(&self, input: &Value) -> bool {
        matches!(
            input.get("action").and_then(Value::as_str),
            Some("create_music") | Some("create_image")
        )
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let result = match action {
            "create_music" => crate::commands::marketing::marketing_creative_create_music(normalize_music_request(&input)?).await,
            "create_image" => crate::commands::marketing::marketing_creative_create_image(normalize_image_request(&input)?).await,
            "get_job" => crate::commands::marketing::marketing_creative_job(job_id(&input)?).await,
            "list_jobs" => crate::commands::marketing::marketing_creative_jobs().await,
            "stage_for_chat" => crate::commands::marketing::marketing_creative_stage_for_chat(job_id(&input)?).await,
            "save_to_workspace" => crate::commands::marketing::marketing_creative_save_to_workspace(
                job_id(&input)?,
                Some(ctx.workspace_root.to_string_lossy().to_string()),
            )
            .await,
            _ => return Ok(ToolResult::err("Unknown harvis_creative action. Use create_music, create_image, get_job, list_jobs, stage_for_chat, or save_to_workspace.")),
        };
        result
            .map(|payload| {
                ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
            })
            .map_err(|error| anyhow::anyhow!(error))
    }
}

#[async_trait]
impl Tool for HarvisImageGenerateTool {
    fn name(&self) -> &str {
        "harvis_image_generate"
    }

    fn description(&self) -> &str {
        "Generate a new image, poster, invitation, campaign visual, or image-to-image variation with Harvis GPT Image2. Text-to-image only needs prompt; reference_ids are optional and only enable image-to-image."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Generate Harvis GPT Image2 images or posters from text, with optional reference images.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": {"type": "string", "description": "Complete visual prompt including subject, composition, typography, lighting, and style."},
                "canvas_preset": {"type": "string", "enum": ["television", "vertical", "store-poster", "phone-fullscreen", "custom"], "default": "vertical"},
                "output_width": {"type": "integer", "minimum": 256, "maximum": 4096},
                "output_height": {"type": "integer", "minimum": 256, "maximum": 4096},
                "reference_ids": {"type": "array", "items": {"type": "string"}, "description": "Optional uploaded Harvis reference image ids. Omit for text-to-image."}
            },
            "additionalProperties": false
        })
    }

    fn metadata(&self) -> ToolMetadata {
        metadata(ToolRisk::External, ToolExecutionMode::Exclusive, true)
    }

    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("prompt is required"))?;
        let references = input
            .get("reference_ids")
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
            .cloned();
        let request = compact_object([
            (
                "mode",
                Some(Value::String(
                    if references.is_some() {
                        "image"
                    } else {
                        "text"
                    }
                    .into(),
                )),
            ),
            ("prompt", Some(Value::String(prompt.to_string()))),
            (
                "canvasPreset",
                Some(Value::String(
                    input
                        .get("canvas_preset")
                        .and_then(Value::as_str)
                        .unwrap_or("vertical")
                        .to_string(),
                )),
            ),
            ("outputWidth", input.get("output_width").cloned()),
            ("outputHeight", input.get("output_height").cloned()),
            ("referenceIds", references.map(Value::Array)),
        ]);
        crate::commands::marketing::marketing_creative_create_image(request)
            .await
            .map(payload_result)
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl Tool for HarvisMusicGenerateTool {
    fn name(&self) -> &str {
        "harvis_music_generate"
    }

    fn description(&self) -> &str {
        "Create instrumental music or BGM with Harvis Suno V5.5 from a finished music-generation prompt. Use for songs, brand-film music, watch campaign BGM, and background music."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Generate Harvis Suno V5.5 music or BGM from a music prompt.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": {"type": "string", "description": "Suno-ready music style prompt."},
                "title": {"type": "string"},
                "duration": {"type": "integer", "minimum": 10, "maximum": 600, "default": 45},
                "mode": {"type": "string", "enum": ["free", "assistant"], "default": "free"}
            },
            "additionalProperties": false
        })
    }

    fn metadata(&self) -> ToolMetadata {
        metadata(ToolRisk::External, ToolExecutionMode::Exclusive, true)
    }

    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("prompt is required"))?;
        let request = compact_object([
            ("prompt", Some(Value::String(prompt.to_string()))),
            ("tags", Some(Value::String(prompt.to_string()))),
            (
                "title",
                input
                    .get("title")
                    .cloned()
                    .or_else(|| Some(Value::String("Harvis 背景音乐".into()))),
            ),
            (
                "duration",
                input
                    .get("duration")
                    .cloned()
                    .or_else(|| Some(Value::from(45))),
            ),
            (
                "mode",
                input
                    .get("mode")
                    .cloned()
                    .or_else(|| Some(Value::String("free".into()))),
            ),
        ]);
        crate::commands::marketing::marketing_creative_create_music(request)
            .await
            .map(payload_result)
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl Tool for HarvisJobStatusTool {
    fn name(&self) -> &str {
        "harvis_job_status"
    }

    fn description(&self) -> &str {
        "Read Harvis creative job progress and completed result URLs. Provide job_id for one job, or omit it to list recent jobs."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Check a Harvis image or music generation job, or list recent jobs.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"job_id": {"type": "string"}},
            "additionalProperties": false
        })
    }

    fn metadata(&self) -> ToolMetadata {
        metadata(ToolRisk::Read, ToolExecutionMode::Parallel, false)
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let result = if input.get("job_id").and_then(Value::as_str).is_some() {
            crate::commands::marketing::marketing_creative_job(job_id(&input)?).await
        } else {
            crate::commands::marketing::marketing_creative_jobs().await
        };
        result.map(payload_result).map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl Tool for HarvisSaveResultTool {
    fn name(&self) -> &str {
        "harvis_save_result"
    }

    fn description(&self) -> &str {
        "Download a completed Harvis image or music result into the current workspace, or archive it into the user's material library for later chat attachment."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Save completed Harvis results to the workspace or material library.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["job_id", "destination"],
            "properties": {
                "job_id": {"type": "string"},
                "destination": {"type": "string", "enum": ["workspace", "material_library"]}
            },
            "additionalProperties": false
        })
    }

    fn metadata(&self) -> ToolMetadata {
        metadata(ToolRisk::Write, ToolExecutionMode::Exclusive, true)
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let id = job_id(&input)?;
        let result = match input.get("destination").and_then(Value::as_str) {
            Some("workspace") => {
                crate::commands::marketing::marketing_creative_save_to_workspace(
                    id,
                    Some(ctx.workspace_root.to_string_lossy().to_string()),
                )
                .await
            }
            Some("material_library") => {
                crate::commands::marketing::marketing_creative_stage_for_chat(id).await
            }
            _ => Err("destination must be workspace or material_library".to_string()),
        };
        result
            .map(|payload| {
                let artifacts = saved_artifacts(&payload);
                payload_result(payload).with_artifacts(artifacts)
            })
            .map_err(anyhow::Error::msg)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_image_request, normalize_music_request, saved_artifacts, HarvisCreativeTool,
        HarvisImageGenerateTool, HarvisMusicGenerateTool,
    };
    use piscis_kernel::agent::tool::{Tool, ToolExposure};
    use serde_json::json;

    #[test]
    fn accepts_a_concise_music_agent_request() {
        let value =
            normalize_music_request(&json!({"request": {"prompt": "elegant piano instrumental"}}))
                .unwrap();
        assert_eq!(value["mode"], "free");
        assert_eq!(value["duration"], 45);
        assert_eq!(value["tags"], "elegant piano instrumental");
    }

    #[test]
    fn maps_legacy_canvas_shape_to_direct_text_to_image() {
        let value = normalize_image_request(&json!({
            "request": {"mode": "reference", "prompt": "luxury watch poster", "canvas": {"preset": "vertical", "width": 1024, "height": 1536}}
        }))
        .unwrap();
        assert_eq!(value["mode"], "image");
        assert_eq!(value["canvasPreset"], "vertical");
        assert_eq!(value["outputWidth"], 1024);
        assert_eq!(value["outputHeight"], 1536);
    }

    #[test]
    fn typed_harvis_tools_are_deferred_and_legacy_tool_is_hidden() {
        assert_eq!(
            HarvisImageGenerateTool.metadata().exposure,
            ToolExposure::Deferred
        );
        assert_eq!(
            HarvisMusicGenerateTool.metadata().exposure,
            ToolExposure::Deferred
        );
        assert_eq!(HarvisCreativeTool.metadata().exposure, ToolExposure::Hidden);
    }

    #[test]
    fn image_generation_only_requires_a_prompt() {
        let schema = HarvisImageGenerateTool.input_schema();
        assert_eq!(schema["required"], json!(["prompt"]));
        assert!(schema["properties"]["reference_ids"].is_object());
    }

    #[test]
    fn saved_workspace_results_become_structured_artifacts() {
        let artifacts = saved_artifacts(&json!({
            "saved": [{
                "fileName": "poster.png",
                "mediaType": "image/png",
                "localPath": "C:\\Harmony\\marketing-assets\\poster.png",
                "byteSize": 42
            }]
        }));
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_type, "image");
        assert_eq!(
            artifacts[0].uri.as_deref(),
            Some("C:\\Harmony\\marketing-assets\\poster.png")
        );
    }
}
