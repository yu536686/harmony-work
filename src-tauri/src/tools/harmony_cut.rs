use crate::harmony_cut::{
    continue_project, health, list_projects, prepare_project, HarmonyCutContinueInput,
    HarmonyCutPrepareInput,
};
use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{Tool, ToolContext, ToolResult};
use serde_json::{json, Value};
use std::borrow::Cow;
use tauri::AppHandle;

pub struct HarmonyCutTool {
    pub app_handle: Option<AppHandle>,
}

fn optional_string(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn format_prepare_result(result: crate::harmony_cut::HarmonyCutPrepareOutput) -> String {
    let marker_files = serde_json::to_string_pretty(&result.marker_files).unwrap_or_default();
    format!(
        "商摄快剪初剪已完成。\n\n\
项目ID：{project_id}\n\
标记工作台：[{workbuddy_url}]({workbuddy_url})\n\
原始本地工作台：{review_url}\n\n\
素材标记文件：\n```json\n{marker_files}\n```\n\n\
请点击标记工作台进入软件内的“商摄快剪”页面。用户保存后，回到对话继续执行后续 CapCut/JianYing 草稿生成流程。",
        project_id = result.project_id,
        workbuddy_url = result.workbuddy_url,
        review_url = result.review_url,
        marker_files = marker_files,
    )
}

fn format_continue_result(result: crate::harmony_cut::HarmonyCutContinueOutput) -> String {
    let preview = serde_json::to_string_pretty(&result.preview).unwrap_or_default();
    let draft = serde_json::to_string_pretty(&result.draft).unwrap_or_default();
    let marker_files = serde_json::to_string_pretty(&result.marker_files).unwrap_or_default();
    format!(
        "商摄快剪后续生成已完成。\n\n\
项目ID：{project_id}\n\n\
预览输出：\n```json\n{preview}\n```\n\n\
草稿输出：\n```json\n{draft}\n```\n\n\
标记文件：\n```json\n{marker_files}\n```",
        project_id = result.project_id,
        preview = preview,
        draft = draft,
        marker_files = marker_files,
    )
}

#[async_trait]
impl Tool for HarmonyCutTool {
    fn name(&self) -> &str {
        "harmony_cut"
    }

    fn description(&self) -> &str {
        "Run the local Harmony Cut commercial-editing workflow. Use it when the user asks to edit product/e-commerce videos from a local material folder, create an initial AI rough cut, open the marking workbench, or continue after the user saved the EDL to generate a CapCut/JianYing editable draft."
    }

    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("AI product-video editing workflow: prepare a local material folder into a Harmony Cut marking workbench, then continue after user review to generate preview and CapCut/JianYing draft.")
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["health", "prepare", "continue", "projects"],
                    "description": "health checks the local service; prepare analyzes a media folder and creates a review workbench; continue resumes after user save and creates preview/draft; projects lists recent Harmony Cut projects."
                },
                "media_dir": {
                    "type": "string",
                    "description": "Local material folder for action=prepare, for example D:\\\\shoot\\\\watch"
                },
                "title": {
                    "type": "string",
                    "description": "Optional project title for action=prepare"
                },
                "requirement": {
                    "type": "string",
                    "description": "Editing requirement/template description for action=prepare"
                },
                "project_id": {
                    "type": "string",
                    "description": "Harmony Cut project id for action=continue. If omitted, Harmony Cut continues the latest project."
                },
                "auto_rotate_landscape": {
                    "type": "boolean",
                    "description": "For action=prepare, rotate landscape clips clockwise when they are camera-orientation mistakes. Default true."
                },
                "skip_preview": {
                    "type": "boolean",
                    "description": "For action=continue, skip preview render."
                },
                "skip_draft": {
                    "type": "boolean",
                    "description": "For action=continue, skip CapCut/JianYing draft creation."
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        match action {
            "health" => {
                let status = health(self.app_handle.as_ref()).await;
                Ok(ToolResult::ok(
                    serde_json::to_string_pretty(&status).unwrap_or_default(),
                ))
            }
            "prepare" => {
                let media_dir = optional_string(&input, "media_dir")
                    .ok_or_else(|| anyhow::anyhow!("media_dir is required for prepare"))?;
                let result = prepare_project(
                    HarmonyCutPrepareInput {
                        media_dir,
                        title: optional_string(&input, "title"),
                        requirement: optional_string(&input, "requirement"),
                        auto_rotate_landscape: input
                            .get("auto_rotate_landscape")
                            .and_then(Value::as_bool),
                    },
                    self.app_handle.as_ref(),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
                Ok(ToolResult::ok(format_prepare_result(result)))
            }
            "continue" => {
                let result = continue_project(
                    HarmonyCutContinueInput {
                        project_id: optional_string(&input, "project_id"),
                        skip_preview: input.get("skip_preview").and_then(Value::as_bool),
                        skip_draft: input.get("skip_draft").and_then(Value::as_bool),
                    },
                    self.app_handle.as_ref(),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
                Ok(ToolResult::ok(format_continue_result(result)))
            }
            "projects" => {
                let projects = list_projects(self.app_handle.as_ref())
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                Ok(ToolResult::ok(
                    serde_json::to_string_pretty(&projects).unwrap_or_default(),
                ))
            }
            _ => Ok(ToolResult::err(
                "Unknown harmony_cut action. Use health, prepare, continue, or projects.",
            )),
        }
    }
}
