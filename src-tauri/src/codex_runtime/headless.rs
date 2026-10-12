use crate::codex_runtime::runner::{CodexAttachment, CodexPermissionMode};
use crate::process::tokio_command;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct CodexExecOptions {
    pub workspace_root: PathBuf,
    pub provider: String,
    pub model: String,
    pub prompt: String,
    pub permission_mode: CodexPermissionMode,
    pub attachments: Vec<CodexAttachment>,
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecCommand {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub stdin: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexExecUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexExecResult {
    pub thread_id: Option<String>,
    pub final_message: String,
    pub usage: CodexExecUsage,
    pub events: Vec<Value>,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CodexExecError {
    #[error("failed to start codex exec: {0}")]
    Start(#[source] std::io::Error),
    #[error("codex exec stdin was not available")]
    MissingStdin,
    #[error("codex exec stdout was not available")]
    MissingStdout,
    #[error("codex exec stderr was not available")]
    MissingStderr,
    #[error("codex exec I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("codex exec was cancelled")]
    Cancelled,
    #[error("codex exec exceeded the 30 minute safety timeout")]
    Timeout,
    #[error("codex exec returned exit code {code:?}: {stderr}")]
    Failed { code: Option<i32>, stderr: String },
}

pub fn build_exec_command(executable: &Path, options: &CodexExecOptions) -> CodexExecCommand {
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--skip-git-repo-check".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "-C".to_string(),
        options.workspace_root.display().to_string(),
        "-m".to_string(),
        options.model.clone(),
        "-c".to_string(),
        format!("model_provider=\"{}\"", options.provider),
    ];
    match options.permission_mode {
        CodexPermissionMode::Restricted => {
            args.extend(["--sandbox".to_string(), "read-only".to_string()]);
        }
        CodexPermissionMode::Approval => {
            args.push("--approve-for-me".to_string());
        }
        CodexPermissionMode::FullAccess => {
            args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        }
    }
    for attachment in &options.attachments {
        if attachment.media_type.starts_with("image/") {
            args.extend(["--image".to_string(), attachment.path.clone()]);
        }
    }
    if let Some(thread_id) = options
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|thread_id| !thread_id.is_empty())
    {
        args.extend(["resume".to_string(), thread_id.to_string(), "-".to_string()]);
    } else {
        args.push("-".to_string());
    }

    let file_context = options
        .attachments
        .iter()
        .filter(|attachment| !attachment.media_type.starts_with("image/"))
        .map(|attachment| format!("- {}: {}", attachment.name, attachment.path))
        .collect::<Vec<_>>();
    let stdin = if file_context.is_empty() {
        options.prompt.clone()
    } else {
        format!(
            "{}\n\n可访问的附件文件：\n{}",
            options.prompt,
            file_context.join("\n")
        )
    };

    CodexExecCommand {
        executable: executable.to_path_buf(),
        args,
        stdin,
    }
}

pub async fn run_codex_exec(
    command: CodexExecCommand,
    codex_home: &Path,
    environment: &HashMap<String, String>,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<CodexExecResult, CodexExecError> {
    run_codex_exec_observed(command, codex_home, environment, cancellation, None).await
}

pub async fn run_codex_exec_observed(
    command: CodexExecCommand,
    codex_home: &Path,
    environment: &HashMap<String, String>,
    cancellation: Option<Arc<AtomicBool>>,
    observer: Option<mpsc::Sender<Value>>,
) -> Result<CodexExecResult, CodexExecError> {
    let mut child = tokio_command(&command.executable)
        .args(&command.args)
        .env("CODEX_HOME", codex_home)
        .envs(environment)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(CodexExecError::Start)?;
    {
        let mut stdin = child.stdin.take().ok_or(CodexExecError::MissingStdin)?;
        stdin.write_all(command.stdin.as_bytes()).await?;
        stdin.shutdown().await?;
    }
    let stdout = child.stdout.take().ok_or(CodexExecError::MissingStdout)?;
    let stderr = child.stderr.take().ok_or(CodexExecError::MissingStderr)?;
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut output = Vec::new();
        while let Some(line) = lines.next_line().await? {
            output.push(line);
        }
        Ok::<String, std::io::Error>(output.join("\n"))
    });
    let stdout_task = tokio::spawn(async move {
        let mut parser = CodexExecJsonlParser::default();
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            parser.push_line(&line);
            if let Some(ref sender) = observer {
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    let _ = sender.try_send(event);
                }
            }
        }
        Ok::<CodexExecJsonlParser, std::io::Error>(parser)
    });
    let mut cancelled = false;
    let started_at = tokio::time::Instant::now();
    let status = loop {
        if cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            cancelled = true;
            let _ = child.kill().await;
            break child.wait().await?;
        }
        if started_at.elapsed() >= std::time::Duration::from_secs(30 * 60) {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stdout_task.await;
            let _ = stderr_task.await;
            return Err(CodexExecError::Timeout);
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let parser = stdout_task
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))??;
    let stderr = stderr_task
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))??;
    if cancelled {
        return Err(CodexExecError::Cancelled);
    }
    if !status.success() {
        return Err(CodexExecError::Failed {
            code: status.code(),
            stderr,
        });
    }
    Ok(parser.finish(stderr))
}

#[derive(Debug, Default)]
pub struct CodexExecJsonlParser {
    thread_id: Option<String>,
    final_message: String,
    usage: CodexExecUsage,
    events: Vec<Value>,
}

impl CodexExecJsonlParser {
    pub fn push_line(&mut self, line: &str) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            return;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("thread.started") => {
                self.thread_id = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            Some("item.completed") => {
                if event
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("agent_message")
                {
                    self.final_message = event
                        .get("item")
                        .and_then(|item| item.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                }
            }
            Some("turn.completed") => {
                if let Some(usage) = event.get("usage") {
                    self.usage = CodexExecUsage {
                        input_tokens: u64_field(usage, "input_tokens"),
                        cached_input_tokens: u64_field(usage, "cached_input_tokens"),
                        output_tokens: u64_field(usage, "output_tokens"),
                    };
                }
            }
            _ => {}
        }
        self.events.push(event);
    }

    pub fn finish(self, stderr: String) -> CodexExecResult {
        CodexExecResult {
            thread_id: self.thread_id,
            final_message: self.final_message,
            usage: self.usage,
            events: self.events,
            stderr,
        }
    }
}

fn u64_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(mode: CodexPermissionMode) -> CodexExecOptions {
        CodexExecOptions {
            workspace_root: PathBuf::from("/work"),
            provider: "deepseek".to_string(),
            model: "deepseek-v4-pro".to_string(),
            prompt: "统计数据".to_string(),
            permission_mode: mode,
            attachments: vec![
                CodexAttachment {
                    path: "/work/chart.png".to_string(),
                    name: "chart.png".to_string(),
                    media_type: "image/png".to_string(),
                },
                CodexAttachment {
                    path: "/work/data.csv".to_string(),
                    name: "data.csv".to_string(),
                    media_type: "text/csv".to_string(),
                },
            ],
            thread_id: None,
        }
    }

    #[test]
    fn builds_noninteractive_exec_without_shell_quoting() {
        let command = build_exec_command(
            Path::new("/bin/codex"),
            &options(CodexPermissionMode::Approval),
        );
        assert!(command
            .args
            .windows(2)
            .any(|args| args == ["--image", "/work/chart.png"]));
        assert!(command.args.contains(&"--approve-for-me".to_string()));
        assert_eq!(command.args.last().map(String::as_str), Some("-"));
        assert!(command.stdin.contains("/work/data.csv"));
        assert!(!command.stdin.contains("chart.png"));
    }

    #[test]
    fn full_access_uses_explicit_headless_bypass_flag() {
        let command = build_exec_command(
            Path::new("codex"),
            &options(CodexPermissionMode::FullAccess),
        );
        assert!(command
            .args
            .contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
    }

    #[test]
    fn resumes_the_bound_codex_thread_without_shell_quoting() {
        let mut options = options(CodexPermissionMode::Approval);
        options.thread_id = Some("019-thread".to_string());
        let command = build_exec_command(Path::new("codex"), &options);
        let resume = command
            .args
            .iter()
            .position(|argument| argument == "resume")
            .expect("resume subcommand");
        assert_eq!(
            command.args.get(resume + 1).map(String::as_str),
            Some("019-thread")
        );
        assert_eq!(command.args.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn parses_jsonl_thread_message_and_usage() {
        let mut parser = CodexExecJsonlParser::default();
        for line in [
            r#"{"type":"thread.started","thread_id":"thread-1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"item-1","type":"agent_message","text":"分析完成"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":120,"cached_input_tokens":20,"output_tokens":30}}"#,
        ] {
            parser.push_line(line);
        }
        let result = parser.finish(String::new());
        assert_eq!(result.thread_id.as_deref(), Some("thread-1"));
        assert_eq!(result.final_message, "分析完成");
        assert_eq!(result.usage.input_tokens, 120);
        assert_eq!(result.usage.output_tokens, 30);
        assert_eq!(result.events.len(), 4);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closes_prompt_stdin_before_waiting_for_exec_to_finish() {
        use std::os::unix::fs::PermissionsExt;

        let script = std::env::temp_dir().join(format!(
            "workbuddy-codex-eof-{}-{}.sh",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &script,
            "#!/bin/sh\ncat >/dev/null\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"eof-test\"}' '{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"ok\"}}' '{\"type\":\"turn.completed\",\"usage\":{}}'\n",
        )
        .expect("write fake codex executable");
        let mut permissions = std::fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("make fake codex executable");

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_codex_exec(
                CodexExecCommand {
                    executable: script.clone(),
                    args: Vec::new(),
                    stdin: "hello".to_string(),
                },
                Path::new("/tmp"),
                &HashMap::new(),
                None,
            ),
        )
        .await;
        let _ = std::fs::remove_file(script);

        let output = result
            .expect("fake exec must observe EOF")
            .expect("fake exec succeeds");
        assert_eq!(output.final_message, "ok");
    }
}
