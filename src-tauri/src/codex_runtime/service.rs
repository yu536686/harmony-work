use crate::codex_runtime::client::{AppServerError, AppServerEvent};
use crate::codex_runtime::config::{CodexRuntimeConfig, DeepSeekProfile, PINNED_CODEX_VERSION};
use crate::codex_runtime::event_adapter::{CodexEventAdapter, WorkBuddyEvent};
use crate::codex_runtime::process::{
    AppServerIdentity, CodexLaunchConfig, CodexProcess, CodexProcessError,
};
use crate::codex_runtime::runner::{
    approval_result, extract_thread_id, extract_turn_id, thread_resume_params, thread_start_params,
    turn_start_params, CodexAttachment, CodexPermissionMode, CodexTurnOptions,
};
use crate::codex_runtime::session_map::{
    CodexSessionMapError, CodexSessionMapStore, NewCodexSessionBinding,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};

#[derive(Clone)]
pub struct CodexRuntimeState {
    process: Arc<Mutex<Option<Arc<CodexProcess>>>>,
    sessions: Arc<CodexSessionMapStore>,
    codex_home: PathBuf,
    executable_override: Option<PathBuf>,
    environment: Arc<Mutex<HashMap<String, String>>>,
    subscribers: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<WorkBuddyEvent>>>>,
    pending_approvals: Arc<Mutex<HashMap<String, Value>>>,
    event_adapter: Arc<Mutex<CodexEventAdapter>>,
}

pub struct CodexTurnExecution {
    pub thread_id: String,
    pub turn_id: String,
    pub events: mpsc::UnboundedReceiver<WorkBuddyEvent>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnStarted {
    pub thread_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeDiagnostic {
    pub running: bool,
    pub display_version: String,
    pub package_version: String,
    pub target_codex_version: String,
    pub executable: Option<String>,
    pub user_agent: Option<String>,
    pub codex_home: String,
    pub version_matches_target: Option<bool>,
    pub mapped_sessions: u64,
    pub stderr_tail: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CodexRuntimeStateError {
    #[error(transparent)]
    Process(#[from] CodexProcessError),
    #[error(transparent)]
    SessionMap(#[from] CodexSessionMapError),
    #[error("Codex app-server request failed: {0}")]
    AppServer(#[from] AppServerError),
    #[error("Codex app-server response did not include {0}")]
    MissingResponseId(&'static str),
    #[error("failed to prepare Codex provider config: {0}")]
    PrepareConfig(#[source] std::io::Error),
    #[error("Codex approval request was not found or expired")]
    ApprovalNotFound,
}

impl CodexRuntimeState {
    pub fn new(app_data_dir: &Path) -> Result<Self, CodexRuntimeStateError> {
        let codex_home = app_data_dir.join("codex-home");
        std::fs::create_dir_all(&codex_home).map_err(CodexRuntimeStateError::PrepareConfig)?;
        let config_path = codex_home.join("config.toml");
        if !config_path.exists() {
            std::fs::write(&config_path, DeepSeekProfile::default().render_toml())
                .map_err(CodexRuntimeStateError::PrepareConfig)?;
        }
        Ok(Self {
            process: Arc::new(Mutex::new(None)),
            sessions: Arc::new(CodexSessionMapStore::open(
                &app_data_dir.join("codex-runtime.db"),
            )?),
            codex_home,
            executable_override: std::env::var_os("XIAOHENG_CODEX_PATH").map(PathBuf::from),
            environment: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            pending_approvals: Arc::new(Mutex::new(HashMap::new())),
            event_adapter: Arc::new(Mutex::new(CodexEventAdapter::default())),
        })
    }

    pub async fn configure_credentials(&self, settings: &crate::store::Settings) {
        let mut environment = self.environment.lock().await;
        environment.clear();
        if !settings.openai_api_key.trim().is_empty() {
            environment.insert(
                "OPENAI_API_KEY".to_string(),
                settings.openai_api_key.clone(),
            );
        }
        if !settings.deepseek_api_key.trim().is_empty() {
            environment.insert(
                "DEEPSEEK_API_KEY".to_string(),
                settings.deepseek_api_key.clone(),
            );
        }
    }

    pub fn sessions(&self) -> Arc<CodexSessionMapStore> {
        Arc::clone(&self.sessions)
    }

    pub async fn ensure_started(&self) -> Result<Arc<CodexProcess>, CodexRuntimeStateError> {
        let mut process = self.process.lock().await;
        if let Some(existing) = process.as_ref() {
            if existing.is_running().await? {
                return Ok(Arc::clone(existing));
            }
            *process = None;
        }
        let launched = Arc::new(
            CodexProcess::launch(CodexLaunchConfig {
                executable_override: self.executable_override.clone(),
                codex_home: self.codex_home.clone(),
                identity: AppServerIdentity::b1(),
                environment: self.environment.lock().await.clone(),
            })
            .await?,
        );
        *process = Some(Arc::clone(&launched));
        drop(process);
        self.spawn_event_dispatcher(Arc::clone(&launched));
        Ok(launched)
    }

    pub async fn start_turn(
        &self,
        workbuddy_session_id: &str,
        options: CodexTurnOptions,
    ) -> Result<CodexTurnExecution, CodexRuntimeStateError> {
        let process = self.ensure_started().await?;
        let client = process.client();
        let existing = self.sessions.get(workbuddy_session_id)?;
        let thread_id = if let Some(binding) = existing {
            match client
                .request(
                    "thread/resume",
                    thread_resume_params(&binding.codex_thread_id, &options),
                )
                .await
            {
                Ok(response) => extract_thread_id(&response)
                    .unwrap_or(&binding.codex_thread_id)
                    .to_string(),
                Err(AppServerError::Rpc(_)) => {
                    let response = client
                        .request("thread/start", thread_start_params(&options))
                        .await?;
                    extract_thread_id(&response)
                        .ok_or(CodexRuntimeStateError::MissingResponseId("thread.id"))?
                        .to_string()
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            let response = client
                .request("thread/start", thread_start_params(&options))
                .await?;
            extract_thread_id(&response)
                .ok_or(CodexRuntimeStateError::MissingResponseId("thread.id"))?
                .to_string()
        };

        self.sessions.bind(NewCodexSessionBinding {
            workbuddy_session_id,
            codex_thread_id: &thread_id,
            workspace_root: &options.workspace_root,
            provider: &options.provider,
            model: &options.model,
        })?;
        let (sender, receiver) = mpsc::unbounded_channel();
        self.subscribers
            .lock()
            .await
            .insert(thread_id.clone(), sender);
        let response = match client
            .request("turn/start", turn_start_params(&thread_id, &options))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.subscribers.lock().await.remove(&thread_id);
                return Err(error.into());
            }
        };
        let turn_id = extract_turn_id(&response)
            .ok_or(CodexRuntimeStateError::MissingResponseId("turn.id"))?
            .to_string();
        Ok(CodexTurnExecution {
            thread_id,
            turn_id,
            events: receiver,
        })
    }

    pub async fn respond_approval(
        &self,
        request_id: &str,
        approved: bool,
        for_session: bool,
    ) -> Result<(), CodexRuntimeStateError> {
        let rpc_id = self
            .pending_approvals
            .lock()
            .await
            .remove(request_id)
            .ok_or(CodexRuntimeStateError::ApprovalNotFound)?;
        self.ensure_started()
            .await?
            .client()
            .respond(rpc_id, approval_result(approved, for_session))
            .await?;
        Ok(())
    }

    pub async fn interrupt_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> Result<(), CodexRuntimeStateError> {
        self.ensure_started()
            .await?
            .client()
            .request(
                "turn/interrupt",
                serde_json::json!({"threadId": thread_id, "turnId": turn_id}),
            )
            .await?;
        Ok(())
    }

    fn spawn_event_dispatcher(&self, process: Arc<CodexProcess>) {
        let state = self.clone();
        tokio::spawn(async move {
            while let Some(event) = process.next_event().await {
                match event {
                    AppServerEvent::Notification(notification) => {
                        let thread_id = notification
                            .params
                            .get("threadId")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let events = state
                            .event_adapter
                            .lock()
                            .await
                            .adapt_notification(&notification);
                        if let Some(thread_id) = thread_id {
                            state.dispatch_events(&thread_id, events).await;
                        }
                    }
                    AppServerEvent::ServerRequest(request) => {
                        let thread_id = request
                            .params
                            .get("threadId")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        if let (Some(thread_id), Some(event)) = (
                            thread_id,
                            state.event_adapter.lock().await.adapt_request(&request),
                        ) {
                            let request_id = rpc_id_key(&request.id);
                            state
                                .pending_approvals
                                .lock()
                                .await
                                .insert(request_id, request.id);
                            state.dispatch_events(&thread_id, vec![event]).await;
                        }
                    }
                    AppServerEvent::ProtocolError(error)
                    | AppServerEvent::TransportError(error) => {
                        state.broadcast_error(error).await;
                    }
                    AppServerEvent::Closed => {
                        state
                            .broadcast_error("Codex app-server 连接已关闭".to_string())
                            .await;
                        state.pending_approvals.lock().await.clear();
                        let mut active = state.process.lock().await;
                        if active
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, &process))
                        {
                            *active = None;
                        }
                        break;
                    }
                }
            }
        });
    }

    async fn dispatch_events(&self, thread_id: &str, events: Vec<WorkBuddyEvent>) {
        let terminal = events.iter().any(is_terminal_event);
        let sender = self.subscribers.lock().await.get(thread_id).cloned();
        if let Some(sender) = sender {
            for event in events {
                let _ = sender.send(event);
            }
        }
        if terminal {
            self.subscribers.lock().await.remove(thread_id);
        }
    }

    async fn broadcast_error(&self, message: String) {
        let subscribers = self
            .subscribers
            .lock()
            .await
            .drain()
            .map(|(_, sender)| sender)
            .collect::<Vec<_>>();
        for sender in subscribers {
            let _ = sender.send(WorkBuddyEvent::Error {
                message: message.clone(),
            });
        }
    }

    pub async fn diagnostic(
        &self,
        start: bool,
    ) -> Result<CodexRuntimeDiagnostic, CodexRuntimeStateError> {
        let process = if start {
            Some(self.ensure_started().await?)
        } else {
            self.process.lock().await.clone()
        };
        let runtime_config = CodexRuntimeConfig::b1_default();
        let mapped_sessions = self.sessions.count()?;
        let (executable, user_agent, version_matches_target, stderr_tail) = match process {
            Some(process) => {
                let user_agent = process
                    .initialization()
                    .get("userAgent")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let version_matches = user_agent
                    .as_deref()
                    .map(|agent| agent.contains(PINNED_CODEX_VERSION));
                (
                    Some(process.executable().display().to_string()),
                    user_agent,
                    version_matches,
                    process.stderr_snapshot().await,
                )
            }
            None => (None, None, None, Vec::new()),
        };

        Ok(CodexRuntimeDiagnostic {
            running: executable.is_some(),
            display_version: runtime_config.display_version.to_string(),
            package_version: runtime_config.package_version.to_string(),
            target_codex_version: runtime_config.codex_version.to_string(),
            executable,
            user_agent,
            codex_home: self.codex_home.display().to_string(),
            version_matches_target,
            mapped_sessions,
            stderr_tail,
        })
    }

    pub async fn shutdown(&self) -> Result<(), CodexRuntimeStateError> {
        let process = self.process.lock().await.take();
        if let Some(process) = process {
            process.shutdown().await?;
        }
        Ok(())
    }
}

fn rpc_id_key(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn is_terminal_event(event: &WorkBuddyEvent) -> bool {
    matches!(
        event,
        WorkBuddyEvent::Done { .. } | WorkBuddyEvent::Cancelled | WorkBuddyEvent::Error { .. }
    )
}

fn parse_permission_mode(value: &str) -> CodexPermissionMode {
    match value {
        "restricted" => CodexPermissionMode::Restricted,
        "full" | "full_access" => CodexPermissionMode::FullAccess,
        _ => CodexPermissionMode::Approval,
    }
}

#[tauri::command]
pub async fn codex_runtime_diagnostic(
    state: tauri::State<'_, CodexRuntimeState>,
    start: Option<bool>,
) -> Result<CodexRuntimeDiagnostic, String> {
    state
        .diagnostic(start.unwrap_or(false))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn codex_chat_send(
    app: tauri::AppHandle,
    state: tauri::State<'_, CodexRuntimeState>,
    session_id: String,
    content: String,
    workspace_root: String,
    provider: String,
    model: String,
    permission_mode: Option<String>,
    attachments: Option<Vec<CodexAttachment>>,
) -> Result<CodexTurnStarted, String> {
    let options = CodexTurnOptions {
        content,
        workspace_root,
        provider,
        model,
        permission_mode: parse_permission_mode(permission_mode.as_deref().unwrap_or("approval")),
        developer_instructions: Some(
            "你是小亨 WorkBuddy。始终使用名称“小亨”，作为用户的桌面 AI 工作伙伴完成任务。"
                .to_string(),
        ),
        attachments: attachments.unwrap_or_default(),
    };
    let execution = state
        .start_turn(&session_id, options)
        .await
        .map_err(|error| error.to_string())?;
    let started = CodexTurnStarted {
        thread_id: execution.thread_id.clone(),
        turn_id: execution.turn_id.clone(),
    };
    let event_name = format!("agent_event_{session_id}");
    let mut events = execution.events;
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let terminal = is_terminal_event(&event);
            let _ = app.emit(&event_name, event);
            if terminal {
                break;
            }
        }
    });
    Ok(started)
}

#[tauri::command]
pub async fn codex_chat_cancel(
    state: tauri::State<'_, CodexRuntimeState>,
    thread_id: String,
    turn_id: String,
) -> Result<(), String> {
    state
        .interrupt_turn(&thread_id, &turn_id)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_stopped_runtime_without_launching_process() {
        let directory = std::env::temp_dir().join(format!("codex-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create state dir");
        let state = CodexRuntimeState::new(&directory).expect("state");

        let diagnostic = state.diagnostic(false).await.expect("diagnostic");
        assert!(!diagnostic.running);
        assert_eq!(diagnostic.display_version, "b1.0.0");
        assert_eq!(diagnostic.target_codex_version, "0.149.0");
        assert_eq!(diagnostic.mapped_sessions, 0);

        drop(state);
        std::fs::remove_dir_all(directory).expect("remove state dir");
    }
}
