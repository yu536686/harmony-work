use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
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

#[async_trait]
impl Tool for HarvisCreativeTool {
    fn name(&self) -> &str {
        "harvis_creative"
    }

    fn description(&self) -> &str {
        "Create Harvis music or images through the authenticated Harmony gateway, check an existing creative job, or archive a completed job into the user's material library. Use only when the user explicitly asks to generate music, background music, an image, a poster, or other visual creative output."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Create Harvis music or images through the logged-in Harmony gateway; query progress and archive completed results into the material library.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create_music", "create_image", "get_job", "list_jobs", "stage_for_chat"],
                    "description": "create_music submits Suno V5.5 instrumental generation; create_image submits GPT Image2; get_job and list_jobs read status; stage_for_chat archives completed assets for the active user."
                },
                "request": {
                    "type": "object",
                    "description": "For create_music: {mode:'free'|'assistant', prompt, tags, title, duration}. For create_image: {mode:'text'|'reference', prompt, canvas:{preset,width?,height?}, referenceIds?:string[]}."
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
            "create_music" => crate::commands::marketing::marketing_creative_create_music(request_object(&input)?).await,
            "create_image" => crate::commands::marketing::marketing_creative_create_image(request_object(&input)?).await,
            "get_job" => crate::commands::marketing::marketing_creative_job(job_id(&input)?).await,
            "list_jobs" => crate::commands::marketing::marketing_creative_jobs().await,
            "stage_for_chat" => crate::commands::marketing::marketing_creative_stage_for_chat(job_id(&input)?).await,
            _ => return Ok(ToolResult::err("Unknown harvis_creative action. Use create_music, create_image, get_job, list_jobs, or stage_for_chat.")),
        };
        result
            .map(|payload| ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default()))
            .map_err(|error| anyhow::anyhow!(error))
    }
}
