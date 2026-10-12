//! Public, bounded task progress derived from Codex JSONL without forwarding private output.
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct PublicProgress {
    pub kind: &'static str,
    pub payload: Value,
}

pub fn map_codex_progress(event: &Value) -> Option<PublicProgress> {
    if event["type"] == "turn.started" {
        return Some(PublicProgress { kind: "planning", payload: json!({}) });
    }
    let kind = match event["type"].as_str()? {
        "item.started" => "tool_started",
        "item.completed" => "tool_finished",
        _ => return None,
    };
    let tool_name = match event["item"]["type"].as_str()? {
        "command_execution" => "终端命令",
        "mcp_tool_call" => "外部工具",
        "web_search" => "网页检索",
        "file_change" => "文件修改",
        _ => return None,
    };
    Some(PublicProgress { kind, payload: json!({ "toolName": tool_name }) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_only_safe_static_progress_without_commands_paths_or_output() {
        let private = json!({
            "type": "item.started", "item": {
                "type": "command_execution", "command": "cat /Users/private/.env",
                "output": "sk-secret-key"
            }
        });
        let progress = map_codex_progress(&private).expect("safe tool event");
        assert_eq!(progress.kind, "tool_started");
        assert_eq!(progress.payload, json!({"toolName":"终端命令"}));
        assert!(!progress.payload.to_string().contains("private"));
        assert!(map_codex_progress(&json!({"type":"item.completed","item":{"type":"reasoning","text":"secret"}})).is_none());
        assert_eq!(map_codex_progress(&json!({"type":"turn.started"})).unwrap().kind, "planning");
    }
}
