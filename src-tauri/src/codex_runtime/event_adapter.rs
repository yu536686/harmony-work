use crate::codex_runtime::protocol::{RpcNotification, RpcRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkBuddyPlanItem {
    pub id: String,
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkBuddyEvent {
    RunStatus {
        iteration: u32,
        phase: String,
        detail: String,
    },
    TextSegmentStart {
        iteration: u32,
    },
    TextDelta {
        delta: String,
    },
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },
    ToolEnd {
        id: String,
        name: String,
        result: String,
        is_error: bool,
    },
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: Value,
        description: String,
    },
    ContextUsage {
        estimated_input_tokens: u32,
        total_input_budget: u32,
        trigger_threshold: u32,
        cumulative_input_tokens: u32,
        cumulative_output_tokens: u32,
        rolling_summary_version: u32,
        auto_compact_threshold: u32,
    },
    Done {
        total_input_tokens: u32,
        total_output_tokens: u32,
    },
    Cancelled,
    Error {
        message: String,
    },
    PlanUpdate {
        items: Vec<WorkBuddyPlanItem>,
    },
}

#[derive(Debug, Clone, Copy, Default)]
struct UsageSnapshot {
    input: u32,
    output: u32,
    context_window: u32,
}

#[derive(Debug, Default)]
pub struct CodexEventAdapter {
    iterations: HashMap<String, u32>,
    usage: HashMap<String, UsageSnapshot>,
}

impl CodexEventAdapter {
    pub fn adapt_notification(&mut self, notification: &RpcNotification) -> Vec<WorkBuddyEvent> {
        match notification.method.as_str() {
            "turn/started" => self.turn_started(&notification.params),
            "item/agentMessage/delta" => notification
                .params
                .get("delta")
                .and_then(Value::as_str)
                .map(|delta| vec![WorkBuddyEvent::TextDelta { delta: delta.into() }])
                .unwrap_or_default(),
            "item/started" => self.item_started(&notification.params),
            "item/completed" => self.item_completed(&notification.params),
            "turn/plan/updated" => self.plan_updated(&notification.params),
            "thread/tokenUsage/updated" => self.token_usage_updated(&notification.params),
            "turn/completed" => self.turn_completed(&notification.params),
            _ => Vec::new(),
        }
    }

    pub fn adapt_request(&self, request: &RpcRequest) -> Option<WorkBuddyEvent> {
        let tool_name = match request.method.as_str() {
            "item/commandExecution/requestApproval" => "shell",
            "item/fileChange/requestApproval" => "apply_patch",
            _ => return None,
        };
        let description = request
            .params
            .get("reason")
            .and_then(Value::as_str)
            .or_else(|| request.params.get("command").and_then(Value::as_str))
            .unwrap_or("需要用户批准后继续执行")
            .to_string();

        Some(WorkBuddyEvent::PermissionRequest {
            request_id: value_id(&request.id),
            tool_name: tool_name.to_string(),
            tool_input: request.params.clone(),
            description,
        })
    }

    fn turn_started(&mut self, params: &Value) -> Vec<WorkBuddyEvent> {
        let thread_id = string_field(params, "threadId").unwrap_or("unknown");
        let iteration = self.iterations.entry(thread_id.to_string()).or_insert(0);
        *iteration = iteration.saturating_add(1);
        vec![
            WorkBuddyEvent::RunStatus {
                iteration: *iteration,
                phase: "thinking".to_string(),
                detail: "正在处理任务".to_string(),
            },
            WorkBuddyEvent::TextSegmentStart {
                iteration: *iteration,
            },
        ]
    }

    fn item_started(&self, params: &Value) -> Vec<WorkBuddyEvent> {
        let Some(item) = params.get("item") else {
            return Vec::new();
        };
        let Some((id, name)) = tool_identity(item) else {
            return Vec::new();
        };
        vec![WorkBuddyEvent::ToolStart {
            id,
            name,
            input: item.clone(),
        }]
    }

    fn item_completed(&self, params: &Value) -> Vec<WorkBuddyEvent> {
        let Some(item) = params.get("item") else {
            return Vec::new();
        };
        let Some((id, name)) = tool_identity(item) else {
            return Vec::new();
        };
        let status_failed = matches!(
            item.get("status").and_then(Value::as_str),
            Some("failed" | "declined")
        );
        let exit_failed = item
            .get("exitCode")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0);
        let result = item
            .get("aggregatedOutput")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| item.to_string());
        vec![WorkBuddyEvent::ToolEnd {
            id,
            name,
            result,
            is_error: status_failed || exit_failed,
        }]
    }

    fn plan_updated(&self, params: &Value) -> Vec<WorkBuddyEvent> {
        let items = params
            .get("plan")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, step)| {
                let content = step.get("step")?.as_str()?.to_string();
                let status = match step.get("status").and_then(Value::as_str).unwrap_or("pending") {
                    "inProgress" => "in_progress",
                    "completed" => "completed",
                    _ => "pending",
                };
                Some(WorkBuddyPlanItem {
                    id: format!("step-{}", index + 1),
                    content,
                    status: status.to_string(),
                })
            })
            .collect();
        vec![WorkBuddyEvent::PlanUpdate { items }]
    }

    fn token_usage_updated(&mut self, params: &Value) -> Vec<WorkBuddyEvent> {
        let thread_id = string_field(params, "threadId").unwrap_or("unknown");
        let token_usage = params.get("tokenUsage").unwrap_or(&Value::Null);
        let total = token_usage.get("total").unwrap_or(&Value::Null);
        let last = token_usage.get("last").unwrap_or(&Value::Null);
        let snapshot = UsageSnapshot {
            input: u32_field(total, "inputTokens"),
            output: u32_field(total, "outputTokens"),
            context_window: u32_field(token_usage, "modelContextWindow"),
        };
        self.usage.insert(thread_id.to_string(), snapshot);
        vec![WorkBuddyEvent::ContextUsage {
            estimated_input_tokens: u32_field(last, "inputTokens"),
            total_input_budget: snapshot.context_window,
            trigger_threshold: snapshot.context_window.saturating_mul(3) / 5,
            cumulative_input_tokens: snapshot.input,
            cumulative_output_tokens: snapshot.output,
            rolling_summary_version: 0,
            auto_compact_threshold: 0,
        }]
    }

    fn turn_completed(&self, params: &Value) -> Vec<WorkBuddyEvent> {
        let thread_id = string_field(params, "threadId").unwrap_or("unknown");
        let iteration = self.iterations.get(thread_id).copied().unwrap_or(1);
        let turn = params.get("turn").unwrap_or(&Value::Null);
        match turn.get("status").and_then(Value::as_str).unwrap_or("failed") {
            "completed" => {
                let usage = self.usage.get(thread_id).copied().unwrap_or_default();
                vec![
                    WorkBuddyEvent::RunStatus {
                        iteration,
                        phase: "closing".to_string(),
                        detail: "任务已完成".to_string(),
                    },
                    WorkBuddyEvent::Done {
                        total_input_tokens: usage.input,
                        total_output_tokens: usage.output,
                    },
                ]
            }
            "interrupted" => vec![WorkBuddyEvent::Cancelled],
            _ => vec![WorkBuddyEvent::Error {
                message: turn
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Codex turn 执行失败")
                    .to_string(),
            }],
        }
    }
}

fn tool_identity(item: &Value) -> Option<(String, String)> {
    let id = item.get("id")?.as_str()?.to_string();
    let name = match item.get("type")?.as_str()? {
        "commandExecution" => "shell",
        "fileChange" => "apply_patch",
        "mcpToolCall" => item
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("mcp_tool"),
        "webSearch" => "web_search",
        _ => return None,
    };
    Some((id, name.to_string()))
}

fn value_id(value: &Value) -> String {
    value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn u32_field(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or_default()
        .min(u32::MAX as u64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_runtime::protocol::{RpcNotification, RpcRequest};
    use serde_json::json;

    #[test]
    fn maps_agent_message_delta_without_narration() {
        let mut adapter = CodexEventAdapter::default();
        let events = adapter.adapt_notification(&RpcNotification {
            method: "item/agentMessage/delta".to_string(),
            params: json!({"threadId":"th1","turnId":"tu1","itemId":"i1","delta":"你好"}),
        });

        assert_eq!(events, vec![WorkBuddyEvent::TextDelta { delta: "你好".into() }]);
    }

    #[test]
    fn maps_command_lifecycle_to_tool_events() {
        let mut adapter = CodexEventAdapter::default();
        let started = adapter.adapt_notification(&RpcNotification {
            method: "item/started".to_string(),
            params: json!({
                "threadId":"th1","turnId":"tu1","startedAtMs":1,
                "item":{"id":"cmd1","type":"commandExecution","command":"npm test","cwd":"/work","status":"inProgress","commandActions":[]}
            }),
        });
        let completed = adapter.adapt_notification(&RpcNotification {
            method: "item/completed".to_string(),
            params: json!({
                "threadId":"th1","turnId":"tu1","completedAtMs":2,
                "item":{"id":"cmd1","type":"commandExecution","command":"npm test","cwd":"/work","status":"completed","commandActions":[],"aggregatedOutput":"39 tests passed","exitCode":0}
            }),
        });

        assert!(matches!(
            started.as_slice(),
            [WorkBuddyEvent::ToolStart { id, name, .. }] if id == "cmd1" && name == "shell"
        ));
        assert!(matches!(
            completed.as_slice(),
            [WorkBuddyEvent::ToolEnd { id, result, is_error, .. }]
                if id == "cmd1" && result == "39 tests passed" && !is_error
        ));
    }

    #[test]
    fn maps_authoritative_plan_and_normalizes_status() {
        let mut adapter = CodexEventAdapter::default();
        let events = adapter.adapt_notification(&RpcNotification {
            method: "turn/plan/updated".to_string(),
            params: json!({
                "threadId":"th1","turnId":"tu1","explanation":null,
                "plan":[
                    {"step":"读取 CSV","status":"inProgress"},
                    {"step":"生成 PPT","status":"pending"}
                ]
            }),
        });

        assert_eq!(
            events,
            vec![WorkBuddyEvent::PlanUpdate {
                items: vec![
                    WorkBuddyPlanItem { id: "step-1".into(), content: "读取 CSV".into(), status: "in_progress".into() },
                    WorkBuddyPlanItem { id: "step-2".into(), content: "生成 PPT".into(), status: "pending".into() },
                ]
            }]
        );
    }

    #[test]
    fn completed_turn_emits_closing_and_done_with_latest_usage() {
        let mut adapter = CodexEventAdapter::default();
        adapter.adapt_notification(&RpcNotification {
            method: "thread/tokenUsage/updated".to_string(),
            params: json!({
                "threadId":"th1","turnId":"tu1",
                "tokenUsage":{
                    "last":{"inputTokens":120,"cachedInputTokens":20,"outputTokens":30,"reasoningOutputTokens":5,"totalTokens":150},
                    "total":{"inputTokens":500,"cachedInputTokens":100,"outputTokens":80,"reasoningOutputTokens":10,"totalTokens":580},
                    "modelContextWindow":128000
                }
            }),
        });
        let events = adapter.adapt_notification(&RpcNotification {
            method: "turn/completed".to_string(),
            params: json!({"threadId":"th1","turn":{"id":"tu1","status":"completed","items":[]}}),
        });

        assert_eq!(
            events,
            vec![
                WorkBuddyEvent::RunStatus { iteration: 1, phase: "closing".into(), detail: "任务已完成".into() },
                WorkBuddyEvent::Done { total_input_tokens: 500, total_output_tokens: 80 }
            ]
        );
    }

    #[test]
    fn maps_command_approval_to_existing_permission_ui_contract() {
        let adapter = CodexEventAdapter::default();
        let event = adapter.adapt_request(&RpcRequest {
            id: json!("approve-1"),
            method: "item/commandExecution/requestApproval".to_string(),
            params: json!({"itemId":"cmd1","command":"npm install","cwd":"/work","reason":"需要安装依赖"}),
        });

        assert_eq!(
            event,
            Some(WorkBuddyEvent::PermissionRequest {
                request_id: "approve-1".into(),
                tool_name: "shell".into(),
                tool_input: json!({"itemId":"cmd1","command":"npm install","cwd":"/work","reason":"需要安装依赖"}),
                description: "需要安装依赖".into(),
            })
        );
    }
}
