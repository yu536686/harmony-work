use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{Tool, ToolContext, ToolResult};
use serde_json::{json, Map, Value};
use std::borrow::Cow;

pub struct HarvisCreativeTool;

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
        Cow::Borrowed("Generate Harvis Suno music or GPT Image2 images, query progress, and archive completed results into the user's material library.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create_music", "create_image", "get_job", "list_jobs", "stage_for_chat"],
                    "description": "create_music submits Suno V5.5; create_image submits GPT Image2; get_job and list_jobs read status; stage_for_chat archives completed results for the active user."
                },
                "request": {
                    "type": "object",
                    "description": "For create_music: {prompt or tags, title?: string, duration?: 45, mode?: 'free'|'assistant'}. For direct text-to-image: {mode:'text', prompt:'...', canvasPreset:'vertical'|'television'|'store-poster'|'phone-fullscreen'|'custom', outputWidth?: number, outputHeight?: number}. Do not require referenceIds for text-to-image. For image-to-image only: {mode:'image', prompt:'...', canvasPreset:'vertical', referenceIds:['uploaded-reference-id']}."
                },
                "job_id": { "type": "string", "description": "Creative job id for get_job or stage_for_chat." }
            }
        })
    }

    fn needs_confirmation(&self, input: &Value) -> bool {
        matches!(
            input.get("action").and_then(Value::as_str),
            Some("create_music") | Some("create_image")
        )
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = input.get("action").and_then(Value::as_str).unwrap_or("");
        let result = match action {
            "create_music" => crate::commands::marketing::marketing_creative_create_music(normalize_music_request(&input)?).await,
            "create_image" => crate::commands::marketing::marketing_creative_create_image(normalize_image_request(&input)?).await,
            "get_job" => crate::commands::marketing::marketing_creative_job(job_id(&input)?).await,
            "list_jobs" => crate::commands::marketing::marketing_creative_jobs().await,
            "stage_for_chat" => crate::commands::marketing::marketing_creative_stage_for_chat(job_id(&input)?).await,
            _ => return Ok(ToolResult::err("Unknown harvis_creative action. Use create_music, create_image, get_job, list_jobs, or stage_for_chat.")),
        };
        result
            .map(|payload| {
                ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
            })
            .map_err(|error| anyhow::anyhow!(error))
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_image_request, normalize_music_request};
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
}
