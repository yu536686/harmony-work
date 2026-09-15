use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexPermissionMode {
    Restricted,
    Approval,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSandboxMode {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl CodexSandboxMode {
    pub const fn sandbox(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::FullAccess => "danger-full-access",
        }
    }
}

impl CodexPermissionMode {
    pub const fn approval_policy(self) -> &'static str {
        match self {
            Self::Restricted => "untrusted",
            Self::Approval => "on-request",
            Self::FullAccess => "never",
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAttachment {
    pub path: String,
    pub name: String,
    pub media_type: String,
}

#[derive(Debug, Clone)]
pub struct CodexTurnOptions {
    pub content: String,
    pub workspace_root: String,
    pub provider: String,
    pub model: String,
    pub permission_mode: CodexPermissionMode,
    pub sandbox_mode: CodexSandboxMode,
    pub confirm_shell_commands: bool,
    pub confirm_file_writes: bool,
    pub developer_instructions: Option<String>,
    pub attachments: Vec<CodexAttachment>,
}

pub fn thread_start_params(options: &CodexTurnOptions) -> Value {
    json!({
        "approvalPolicy": options.permission_mode.approval_policy(),
        "approvalsReviewer": "user",
        "cwd": options.workspace_root,
        "developerInstructions": options.developer_instructions,
        "ephemeral": false,
        "model": options.model,
        "modelProvider": options.provider,
        "personality": "pragmatic",
        "sandbox": options.sandbox_mode.sandbox(),
        "threadSource": "harmony-work"
    })
}

pub fn thread_resume_params(thread_id: &str, options: &CodexTurnOptions) -> Value {
    json!({
        "threadId": thread_id,
        "approvalPolicy": options.permission_mode.approval_policy(),
        "approvalsReviewer": "user",
        "cwd": options.workspace_root,
        "developerInstructions": options.developer_instructions,
        "model": options.model,
        "modelProvider": options.provider,
        "personality": "pragmatic",
        "sandbox": options.sandbox_mode.sandbox()
    })
}

pub fn turn_start_params(thread_id: &str, options: &CodexTurnOptions) -> Value {
    let mut input = vec![json!({"type": "text", "text": options.content})];
    input.extend(options.attachments.iter().map(|attachment| {
        if attachment.media_type.starts_with("image/") {
            json!({"type": "localImage", "path": attachment.path})
        } else {
            json!({"type": "mention", "name": attachment.name, "path": attachment.path})
        }
    }));
    json!({
        "threadId": thread_id,
        "input": input,
        "approvalPolicy": options.permission_mode.approval_policy(),
        "cwd": options.workspace_root,
        "sandbox": options.sandbox_mode.sandbox(),
        "model": options.model
    })
}

pub fn extract_thread_id(response: &Value) -> Option<&str> {
    response
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
}

pub fn extract_turn_id(response: &Value) -> Option<&str> {
    response
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
}

pub fn approval_result(approved: bool, for_session: bool) -> Value {
    let decision = match (approved, for_session) {
        (true, true) => "acceptForSession",
        (true, false) => "accept",
        (false, _) => "decline",
    };
    json!({"decision": decision})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(mode: CodexPermissionMode) -> CodexTurnOptions {
        CodexTurnOptions {
            content: "分析附件".to_string(),
            workspace_root: "/work/project".to_string(),
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            permission_mode: mode,
            sandbox_mode: match mode {
                CodexPermissionMode::Restricted => CodexSandboxMode::ReadOnly,
                CodexPermissionMode::Approval => CodexSandboxMode::WorkspaceWrite,
                CodexPermissionMode::FullAccess => CodexSandboxMode::FullAccess,
            },
            confirm_shell_commands: false,
            confirm_file_writes: true,
            developer_instructions: Some("你是小亨，运行于 Harmony Work".to_string()),
            attachments: vec![
                CodexAttachment {
                    path: "/work/reference.png".to_string(),
                    name: "reference.png".to_string(),
                    media_type: "image/png".to_string(),
                },
                CodexAttachment {
                    path: "/work/data.csv".to_string(),
                    name: "data.csv".to_string(),
                    media_type: "text/csv".to_string(),
                },
            ],
        }
    }

    #[test]
    fn maps_workbuddy_permission_modes_to_codex_contract() {
        assert_eq!(
            thread_start_params(&options(CodexPermissionMode::Restricted))["sandbox"],
            "read-only"
        );
        assert_eq!(
            thread_start_params(&options(CodexPermissionMode::Approval))["approvalPolicy"],
            "on-request"
        );
        let full = thread_start_params(&options(CodexPermissionMode::FullAccess));
        assert_eq!(full["sandbox"], "danger-full-access");
        assert_eq!(full["approvalPolicy"], "never");
    }

    #[test]
    fn turn_contains_only_new_text_and_native_attachments() {
        let params = turn_start_params("thread-1", &options(CodexPermissionMode::Approval));
        assert_eq!(params["input"].as_array().expect("input").len(), 3);
        assert_eq!(params["input"][0], json!({"type":"text","text":"分析附件"}));
        assert_eq!(params["input"][1]["type"], "localImage");
        assert_eq!(params["input"][2]["type"], "mention");
        assert!(params.get("history").is_none());
        assert!(params.get("messages").is_none());
    }

    #[test]
    fn extracts_v2_thread_and_turn_ids() {
        assert_eq!(
            extract_thread_id(&json!({"thread":{"id":"th1"}})),
            Some("th1")
        );
        assert_eq!(extract_turn_id(&json!({"turn":{"id":"tu1"}})), Some("tu1"));
    }

    #[test]
    fn approval_can_be_scoped_to_single_action_or_session() {
        assert_eq!(approval_result(true, false), json!({"decision":"accept"}));
        assert_eq!(
            approval_result(true, true),
            json!({"decision":"acceptForSession"})
        );
        assert_eq!(approval_result(false, true), json!({"decision":"decline"}));
    }

    #[test]
    fn full_filesystem_access_is_independent_from_confirmation_policy() {
        let mut opts = options(CodexPermissionMode::Approval);
        opts.sandbox_mode = CodexSandboxMode::FullAccess;
        let params = thread_start_params(&opts);
        assert_eq!(params["approvalPolicy"], "on-request");
        assert_eq!(params["sandbox"], "danger-full-access");
    }
}
