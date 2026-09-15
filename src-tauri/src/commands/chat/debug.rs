use crate::codex_runtime::background::{run_codex_background, CodexBackgroundOptions};
use crate::codex_runtime::runner::CodexPermissionMode;
use crate::commands::platform::system::{collect_system_dependencies, SystemDependencyItem};
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugScenario {
    pub id: String,
    pub name: String,
    pub description: String,
    pub name_en: String,
    pub description_en: String,
    pub prompt: String,
    pub expected_keywords: Vec<String>,
    pub expected_tools: Vec<String>,
    #[serde(default)]
    pub requires_config: Option<Vec<String>>,
    #[serde(default)]
    pub platforms: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub input_summary: String,
    pub result_summary: String,
    pub is_error: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario_id: String,
    pub scenario_name: String,
    pub passed: bool,
    pub response_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub missing_keywords: Vec<String>,
    pub missing_tools: Vec<String>,
    pub unexpected_tool_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugReport {
    pub timestamp: String,
    pub system_info: SystemInfo,
    pub settings_summary: SettingsSummary,
    pub system_dependencies: Vec<SystemDependencyItem>,
    pub available_tools: Vec<String>,
    pub recent_audit: Vec<crate::store::db::AuditEntry>,
    pub recent_errors: Vec<String>,
    pub log_tail: Vec<String>,
    pub scenario_results: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub provider: String,
    pub model: String,
    pub workspace_root: String,
    pub policy_mode: String,
    pub max_iterations: u32,
    pub tool_rate_limit: u32,
    pub api_key_configured: bool,
    pub vision_enabled: bool,
    pub vision_configured: bool,
    pub vision_uses_separate_model: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSummary {
    pub provider: String,
    pub model: String,
    pub workspace_root: String,
    pub policy_mode: String,
    pub max_tokens: u32,
    pub max_iterations: u32,
    pub confirm_shell: bool,
    pub confirm_file_write: bool,
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
}

fn scenarios() -> Vec<DebugScenario> {
    vec![
        DebugScenario {
            id: "codex_identity".into(),
            name: "Codex 主链路".into(),
            description: "验证 Codex exec 与小亨身份提示是否生效".into(),
            name_en: "Codex Main Path".into(),
            description_en: "Verify Codex exec and Xiaoheng identity".into(),
            prompt: "只回复：小亨（Harmony Work）Codex 链路正常".into(),
            expected_keywords: vec!["小亨".into(), "Codex".into()],
            expected_tools: vec![],
            requires_config: None,
            platforms: None,
        },
        DebugScenario {
            id: "mcp_memory".into(),
            name: "MCP 记忆工具".into(),
            description: "验证 WorkBuddy MCP Host 的记忆检索工具".into(),
            name_en: "MCP Memory Tool".into(),
            description_en: "Verify WorkBuddy MCP memory search".into(),
            prompt: "调用 memory_search 搜索 WorkBuddy，然后简要报告结果。".into(),
            expected_keywords: vec![],
            expected_tools: vec!["memory_search".into()],
            requires_config: None,
            platforms: None,
        },
        DebugScenario {
            id: "file_roundtrip".into(),
            name: "文件工具往返".into(),
            description: "验证 Codex 原生文件写入与读取".into(),
            name_en: "File Round Trip".into(),
            description_en: "Verify Codex native file write and read".into(),
            prompt: "在当前工作区创建 debug_workbuddy.txt，写入 WORKBUDDY_OK，再读取确认，最后回复 WORKBUDDY_OK。".into(),
            expected_keywords: vec!["WORKBUDDY_OK".into()],
            expected_tools: vec!["shell".into()],
            requires_config: None,
            platforms: None,
        },
        DebugScenario {
            id: "im_channels".into(),
            name: "IM 通道".into(),
            description: "验证企业微信与微信 iLink 通道查询".into(),
            name_en: "IM Channels".into(),
            description_en: "Verify WeCom and WeChat iLink channel discovery".into(),
            prompt: "调用 im_channel_list，确认只展示 wecom 与 wechat。".into(),
            expected_keywords: vec![],
            expected_tools: vec!["im_channel_list".into()],
            requires_config: None,
            platforms: None,
        },
    ]
}

#[tauri::command]
pub async fn list_debug_scenarios() -> Result<Vec<DebugScenario>, String> {
    Ok(scenarios())
}

fn tool_name(item: &Value) -> Option<String> {
    match item.get("type").and_then(Value::as_str) {
        Some("command_execution") => Some("shell".into()),
        Some("mcp_tool_call") => item
            .get("tool")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        Some("web_search") => Some("web_search".into()),
        _ => item.get("name").and_then(Value::as_str).map(str::to_string),
    }
}

fn collect_tool_calls(events: &[Value]) -> Vec<ToolCallRecord> {
    let mut started = HashMap::new();
    let mut calls = Vec::new();
    for event in events {
        let Some(item) = event.get("item") else {
            continue;
        };
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        match event.get("type").and_then(Value::as_str) {
            Some("item.started") => {
                started.insert(id, Instant::now());
            }
            Some("item.completed") => {
                let Some(name) = tool_name(item) else {
                    continue;
                };
                let is_error = item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| matches!(status, "failed" | "declined"))
                    || item
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|code| code != 0);
                calls.push(ToolCallRecord {
                    tool_name: name,
                    input_summary: item
                        .get("command")
                        .or_else(|| item.get("arguments"))
                        .map(Value::to_string)
                        .unwrap_or_default()
                        .chars()
                        .take(240)
                        .collect(),
                    result_summary: item
                        .get("aggregated_output")
                        .or_else(|| item.get("result"))
                        .map(Value::to_string)
                        .unwrap_or_default()
                        .chars()
                        .take(500)
                        .collect(),
                    is_error,
                    duration_ms: started
                        .remove(&id)
                        .map(|start| start.elapsed().as_millis() as u64)
                        .unwrap_or_default(),
                });
            }
            _ => {}
        }
    }
    calls
}

#[tauri::command]
pub async fn run_debug_scenario(
    state: State<'_, AppState>,
    scenario_id: String,
) -> Result<ScenarioResult, String> {
    let scenario = scenarios()
        .into_iter()
        .find(|scenario| scenario.id == scenario_id)
        .ok_or_else(|| format!("Unknown scenario: {scenario_id}"))?;
    let started = Instant::now();
    let session_id = format!("debug_codex_{}", uuid::Uuid::new_v4());
    let result = run_codex_background(
        &state,
        &session_id,
        &scenario.prompt,
        None,
        "debug",
        CodexBackgroundOptions {
            session_title: Some(format!("[调试] {}", scenario.name)),
            session_source: Some("debug_codex".into()),
            permission_mode: Some(CodexPermissionMode::FullAccess),
            extra_context: Some("这是新 Codex 架构诊断任务。直接执行，不要创建额外计划。".into()),
            ..Default::default()
        },
    )
    .await;
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(output) => {
            let tool_calls = collect_tool_calls(&output.events);
            let missing_keywords = scenario
                .expected_keywords
                .iter()
                .filter(|keyword| !output.final_message.contains(keyword.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            let missing_tools = scenario
                .expected_tools
                .iter()
                .filter(|expected| !tool_calls.iter().any(|call| call.tool_name == **expected))
                .cloned()
                .collect::<Vec<_>>();
            let unexpected_tool_errors = tool_calls
                .iter()
                .filter(|call| call.is_error)
                .map(|call| format!("{}: {}", call.tool_name, call.result_summary))
                .collect::<Vec<_>>();
            Ok(ScenarioResult {
                scenario_id: scenario.id,
                scenario_name: scenario.name,
                passed: missing_keywords.is_empty()
                    && missing_tools.is_empty()
                    && unexpected_tool_errors.is_empty(),
                response_text: output.final_message,
                tool_calls,
                error: None,
                duration_ms,
                input_tokens: output.usage.input_tokens.min(u32::MAX as u64) as u32,
                output_tokens: output.usage.output_tokens.min(u32::MAX as u64) as u32,
                missing_keywords,
                missing_tools,
                unexpected_tool_errors,
            })
        }
        Err(error) => Ok(ScenarioResult {
            scenario_id: scenario.id,
            scenario_name: scenario.name,
            passed: false,
            response_text: String::new(),
            tool_calls: vec![],
            error: Some(error),
            duration_ms,
            input_tokens: 0,
            output_tokens: 0,
            missing_keywords: scenario.expected_keywords,
            missing_tools: scenario.expected_tools,
            unexpected_tool_errors: vec![],
        }),
    }
}

#[tauri::command]
pub async fn run_all_debug_scenarios(
    state: State<'_, AppState>,
) -> Result<Vec<ScenarioResult>, String> {
    let mut results = Vec::new();
    for scenario in scenarios() {
        results.push(run_debug_scenario(state.clone(), scenario.id).await?);
    }
    Ok(results)
}

#[tauri::command]
pub async fn get_debug_report(state: State<'_, AppState>) -> Result<DebugReport, String> {
    let (settings_summary, system_info, system_dependencies) = {
        let settings = state.settings.lock().await;
        let enabled_tools = settings
            .builtin_tool_enabled
            .iter()
            .filter(|(_, enabled)| **enabled)
            .map(|(name, _)| name.clone())
            .collect();
        let disabled_tools = settings
            .builtin_tool_enabled
            .iter()
            .filter(|(_, enabled)| !**enabled)
            .map(|(name, _)| name.clone())
            .collect();
        let summary = SettingsSummary {
            provider: settings.provider.clone(),
            model: settings.model.clone(),
            workspace_root: settings.workspace_root.clone(),
            policy_mode: settings.policy_mode.clone(),
            max_tokens: settings.max_tokens,
            max_iterations: settings.max_iterations,
            confirm_shell: settings.confirm_shell_commands,
            confirm_file_write: settings.confirm_file_writes,
            enabled_tools,
            disabled_tools,
        };
        let vision_uses_separate_model = !settings.vision_use_main_llm;
        let info = SystemInfo {
            os: std::env::consts::OS.into(),
            provider: settings.provider.clone(),
            model: settings.model.clone(),
            workspace_root: settings.workspace_root.clone(),
            policy_mode: settings.policy_mode.clone(),
            max_iterations: settings.max_iterations,
            tool_rate_limit: settings.tool_rate_limit_per_minute,
            api_key_configured: settings.is_configured(),
            vision_enabled: settings.vision_enabled,
            vision_configured: settings.vision_enabled
                || (!settings.vision_provider.is_empty()
                    && !settings.vision_model.is_empty()
                    && !settings.vision_api_key.is_empty()),
            vision_uses_separate_model,
        };
        (summary, info, collect_system_dependencies(&settings))
    };
    let recent_audit = state
        .db
        .lock()
        .await
        .get_audit_log(None, None, 20, 0)
        .unwrap_or_default();
    let recent_errors = recent_audit
        .iter()
        .filter(|entry| entry.is_error)
        .map(|entry| {
            format!(
                "[{}] {} - {}",
                entry.tool_name,
                entry.action,
                entry.result_summary.as_deref().unwrap_or("(no detail)")
            )
        })
        .collect();
    Ok(DebugReport {
        timestamp: chrono::Utc::now().to_rfc3339(),
        system_info,
        settings_summary,
        system_dependencies,
        available_tools: vec![
            "codex.exec".into(),
            "memory_search".into(),
            "memory_append".into(),
            "skill_list".into(),
            "skill_manage".into(),
            "harvis_image_generate".into(),
            "harvis_music_generate".into(),
            "document_artifact".into(),
            "presentation_artifact".into(),
            "spreadsheet_artifact".into(),
            "im_channel_list".into(),
            "im_send_message".into(),
            "staff_list".into(),
            "staff_task_create".into(),
        ],
        recent_audit,
        recent_errors,
        log_tail: read_log_tail(50),
        scenario_results: vec![],
    })
}

#[tauri::command]
pub async fn get_log_tail(lines: Option<usize>) -> Result<Vec<String>, String> {
    Ok(read_log_tail(lines.unwrap_or(100)))
}

fn read_log_tail(lines: usize) -> Vec<String> {
    let log_file = crate::app::logging::log_dir().join("workbuddy.latest.log");
    match std::fs::read_to_string(&log_file) {
        Ok(content) => {
            let content = content.lines().map(str::to_string).collect::<Vec<_>>();
            content[content.len().saturating_sub(lines)..].to_vec()
        }
        Err(error) => vec![format!(
            "Log file unavailable ({}): {error}",
            log_file.display()
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_tool_calls, scenarios};
    use serde_json::json;

    #[test]
    fn exposes_codex_native_scenarios() {
        let scenarios = scenarios();
        assert!(scenarios
            .iter()
            .any(|scenario| scenario.id == "codex_identity"));
        assert!(scenarios.iter().any(|scenario| scenario.id == "mcp_memory"));
    }

    #[test]
    fn parses_codex_exec_tool_events() {
        let calls = collect_tool_calls(&[
            json!({"type":"item.started","item":{"id":"1","type":"command_execution","command":"pwd"}}),
            json!({"type":"item.completed","item":{"id":"1","type":"command_execution","command":"pwd","status":"completed","exit_code":0,"aggregated_output":"ok"}}),
        ]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "shell");
        assert!(!calls[0].is_error);
    }
}
