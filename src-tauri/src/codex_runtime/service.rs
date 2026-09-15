use crate::codex_runtime::client::{AppServerError, AppServerEvent};
use crate::codex_runtime::config::{
    merge_managed_mcp_config, CodexRuntimeConfig, WorkBuddyMcpProfile,
    PINNED_CODEX_VERSION,
};
use crate::codex_runtime::event_adapter::{CodexEventAdapter, WorkBuddyEvent, WorkBuddyPlanItem};
use crate::codex_runtime::headless::{
    build_exec_command, run_codex_exec, CodexExecError, CodexExecOptions, CodexExecResult,
};
use crate::codex_runtime::process::{
    resolve_codex_executable, AppServerIdentity, CodexLaunchConfig, CodexProcess, CodexProcessError,
};
use crate::codex_runtime::runner::{
    approval_result, extract_thread_id, extract_turn_id, thread_resume_params, thread_start_params,
    turn_start_params, CodexAttachment, CodexPermissionMode, CodexSandboxMode, CodexTurnOptions,
};
use crate::codex_runtime::session_map::{
    CodexSessionMapError, CodexSessionMapStore, NewCodexSessionBinding,
};
use crate::store::AppState;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveCodexTurn {
    thread_id: String,
    turn_id: String,
}

#[derive(Debug, Clone, Copy)]
struct ApprovalPreferences {
    confirm_shell_commands: bool,
    confirm_file_writes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModelOverride {
    Current { model: String },
    Explicit { provider: String, model: String },
    Named { provider_id: String, model: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyChatAttachment {
    #[serde(alias = "media_type")]
    pub media_type: String,
    pub path: Option<String>,
    pub data: Option<String>,
    pub filename: Option<String>,
}

#[derive(Debug, Default)]
struct CodexTurnTranscript {
    assistant_text: String,
    tool_calls: Vec<Value>,
    tool_results: Vec<Value>,
    plan_items: Vec<WorkBuddyPlanItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexArtifactCandidate {
    name: String,
    artifact_type: String,
    uri: String,
    summary: String,
    metadata_json: Option<String>,
}

impl CodexTurnTranscript {
    fn observe(&mut self, event: &WorkBuddyEvent) {
        match event {
            WorkBuddyEvent::TextDelta { delta } => self.assistant_text.push_str(delta),
            WorkBuddyEvent::ToolStart { id, name, input } => {
                self.tool_calls.push(serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                }));
            }
            WorkBuddyEvent::ToolEnd {
                id,
                name,
                result,
                is_error,
            } => {
                self.tool_results.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "tool_name": name,
                    "content": result,
                    "is_error": is_error,
                }));
            }
            WorkBuddyEvent::PlanUpdate { items } => self.plan_items.clone_from(items),
            _ => {}
        }
    }

    fn projected_tool_calls(&self) -> Vec<Value> {
        let mut calls = self.tool_calls.clone();
        if !self.plan_items.is_empty() {
            calls.push(serde_json::json!({
                "type": "tool_use",
                "id": "codex-plan",
                "name": "plan_todo",
                "input": {
                    "merge": false,
                    "todos": self.plan_items,
                },
            }));
        }
        calls
    }

    fn projected_tool_results(&self) -> Vec<Value> {
        let mut results = self.tool_results.clone();
        if !self.plan_items.is_empty() {
            results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": "codex-plan",
                "tool_name": "plan_todo",
                "content": "Codex 计划已同步",
                "is_error": false,
            }));
        }
        results
    }

    fn tool_calls_json(&self) -> Option<String> {
        let calls = self.projected_tool_calls();
        (!calls.is_empty())
            .then(|| serde_json::to_string(&calls).expect("serialize Codex tool calls"))
    }

    fn tool_results_json(&self) -> Option<String> {
        let results = self.projected_tool_results();
        (!results.is_empty())
            .then(|| serde_json::to_string(&results).expect("serialize Codex tool results"))
    }
}

fn extract_artifact_candidates(result: &str) -> Vec<CodexArtifactCandidate> {
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    collect_artifact_candidates(&value, &mut candidates);
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.uri.clone()));
    candidates
}

fn collect_artifact_candidates(value: &Value, candidates: &mut Vec<CodexArtifactCandidate>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_artifact_candidates(item, candidates);
            }
        }
        Value::Object(object) => {
            let resource_link = object.get("type").and_then(Value::as_str) == Some("resource_link");
            let uri = object
                .get("localPath")
                .or_else(|| object.get("uri"))
                .or_else(|| object.get("path"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let has_artifact_shape = resource_link
                || object.contains_key("artifactType")
                || object.contains_key("artifact_type")
                || object.contains_key("localPath");
            if has_artifact_shape {
                if let Some(uri) = uri {
                    let name = object
                        .get("name")
                        .or_else(|| object.get("fileName"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .or_else(|| {
                            Path::new(uri)
                                .file_name()
                                .and_then(|value| value.to_str())
                                .map(str::to_string)
                        })
                        .unwrap_or_else(|| "WorkBuddy 产物".to_string());
                    let media_type = object
                        .get("mediaType")
                        .or_else(|| object.get("mimeType"))
                        .and_then(Value::as_str);
                    let artifact_type = object
                        .get("artifactType")
                        .or_else(|| object.get("artifact_type"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| artifact_type_from_media(media_type));
                    candidates.push(CodexArtifactCandidate {
                        name,
                        artifact_type,
                        uri: uri.to_string(),
                        summary: "由 WorkBuddy MCP Host 生成并验证".to_string(),
                        metadata_json: serde_json::to_string(object).ok(),
                    });
                }
            }
            for nested in object.values() {
                collect_artifact_candidates(nested, candidates);
            }
        }
        _ => {}
    }
}

fn extract_staff_task_id(result: &str) -> Option<String> {
    fn visit(value: &Value) -> Option<String> {
        match value {
            Value::Array(items) => items.iter().find_map(visit),
            Value::Object(object) => {
                let is_task = object.contains_key("ownerId")
                    && object.contains_key("title")
                    && object.contains_key("status");
                if is_task {
                    return object.get("id").and_then(Value::as_str).map(str::to_string);
                }
                object.values().find_map(visit)
            }
            Value::String(text) => serde_json::from_str::<Value>(text)
                .ok()
                .as_ref()
                .and_then(visit),
            _ => None,
        }
    }

    serde_json::from_str::<Value>(result)
        .ok()
        .as_ref()
        .and_then(visit)
}

fn artifact_type_from_media(media_type: Option<&str>) -> String {
    match media_type.unwrap_or_default() {
        value if value.starts_with("image/") => "image",
        value if value.starts_with("audio/") => "audio",
        value if value.starts_with("video/") => "video",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "document",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            "presentation"
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "spreadsheet",
        _ => "file",
    }
    .to_string()
}

#[derive(Clone)]
pub struct CodexRuntimeState {
    process: Arc<Mutex<Option<Arc<CodexProcess>>>>,
    sessions: Arc<CodexSessionMapStore>,
    codex_home: PathBuf,
    executable_override: Option<PathBuf>,
    environment: Arc<Mutex<HashMap<String, String>>>,
    subscribers: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<WorkBuddyEvent>>>>,
    pending_approvals: Arc<Mutex<HashMap<String, Value>>>,
    approval_preferences: Arc<Mutex<HashMap<String, ApprovalPreferences>>>,
    event_adapter: Arc<Mutex<CodexEventAdapter>>,
    active_turns: Arc<Mutex<HashMap<String, ActiveCodexTurn>>>,
    active_execs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
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
    #[error(transparent)]
    Exec(#[from] CodexExecError),
    #[error("Codex app-server request failed: {0}")]
    AppServer(#[from] AppServerError),
    #[error("Codex app-server response did not include {0}")]
    MissingResponseId(&'static str),
    #[error("failed to prepare Codex provider config: {0}")]
    PrepareConfig(#[source] std::io::Error),
    #[error("Codex approval request was not found or expired")]
    ApprovalNotFound,
    #[error("session already has an active Codex turn")]
    TurnAlreadyActive,
}

impl CodexRuntimeState {
    pub fn new(app_data_dir: &Path) -> Result<Self, CodexRuntimeStateError> {
        let codex_home = app_data_dir.join("codex-home");
        std::fs::create_dir_all(&codex_home).map_err(CodexRuntimeStateError::PrepareConfig)?;
        let config_path = codex_home.join("config.toml");
        let mut existing_config = if config_path.exists() {
            std::fs::read_to_string(&config_path).map_err(CodexRuntimeStateError::PrepareConfig)?
        } else {
            crate::codex_runtime::config::render_default_providers_toml()
        };
        let migrated_config = crate::codex_runtime::config::migrate_public_service_config(&existing_config);
        let public_url_changed = migrated_config != existing_config;
        existing_config = migrated_config;
        // 升级场景：旧版 codex.toml 只有 DeepSeek 段，需补齐 Harmony 内置 provider 段，
        // 否则切换到 Harmony 时 codex 会报 `Model provider harmony not found`。
        let harmony_missing = !existing_config.contains("[model_providers.harmony]");
        if harmony_missing {
            existing_config
                .push_str(&crate::codex_runtime::config::render_harmony_provider_section());
        }
        let mcp_host = resolve_workbuddy_mcp_host();
        let skill_root = app_data_dir.join("skills");
        let installed_skill_root = app_data_dir.join("installed-skills");
        let marketing_base_url = crate::service_urls::marketing_base_url();
        let mcp_profile = WorkBuddyMcpProfile::new(
            &mcp_host,
            app_data_dir,
            &[skill_root.as_path(), installed_skill_root.as_path()],
            &marketing_base_url,
        );
        let merged_config = merge_managed_mcp_config(&existing_config, &mcp_profile.render_toml());
        if public_url_changed || harmony_missing || merged_config != existing_config {
            std::fs::write(&config_path, merged_config)
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
            approval_preferences: Arc::new(Mutex::new(HashMap::new())),
            event_adapter: Arc::new(Mutex::new(CodexEventAdapter::default())),
            active_turns: Arc::new(Mutex::new(HashMap::new())),
            active_execs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn configure_credentials(&self, settings: &crate::store::Settings) {
        let mut next_environment = HashMap::new();
        if !settings.openai_api_key.trim().is_empty() {
            next_environment.insert(
                "OPENAI_API_KEY".to_string(),
                settings.openai_api_key.clone(),
            );
        }
        if !settings.deepseek_api_key.trim().is_empty() {
            next_environment.insert(
                "DEEPSEEK_API_KEY".to_string(),
                settings.deepseek_api_key.clone(),
            );
        }
        // Harmony 内部网关 API Key（走 Codex Responses 协议）
        if !settings.harmony_api_key.trim().is_empty() {
            next_environment.insert(
                "HARMONY_API_KEY".to_string(),
                settings.harmony_api_key.clone(),
            );
        }
        let changed = {
            let mut environment = self.environment.lock().await;
            if *environment == next_environment {
                false
            } else {
                *environment = next_environment;
                true
            }
        };
        if changed {
            if let Some(process) = self.process.lock().await.take() {
                let _ = process.shutdown().await;
            }
        }
    }

    pub fn sessions(&self) -> Arc<CodexSessionMapStore> {
        Arc::clone(&self.sessions)
    }

    pub async fn execute_headless(
        &self,
        workbuddy_session_id: &str,
        mut options: CodexExecOptions,
    ) -> Result<CodexExecResult, CodexRuntimeStateError> {
        if options.thread_id.is_none() {
            options.thread_id = self
                .sessions
                .get(workbuddy_session_id)?
                .map(|binding| binding.codex_thread_id);
        }
        let executable = resolve_codex_executable(self.executable_override.clone())?;
        let command = build_exec_command(&executable, &options);
        let environment = self.environment.lock().await.clone();
        let cancellation = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self
            .active_execs
            .lock()
            .await
            .insert(workbuddy_session_id.to_string(), cancellation.clone())
        {
            previous.store(true, Ordering::Relaxed);
        }
        let result =
            run_codex_exec(command, &self.codex_home, &environment, Some(cancellation)).await;
        self.active_execs.lock().await.remove(workbuddy_session_id);
        let result = result?;
        if let Some(thread_id) = result.thread_id.as_deref() {
            let workspace_root = options.workspace_root.display().to_string();
            self.sessions.bind(NewCodexSessionBinding {
                workbuddy_session_id,
                codex_thread_id: thread_id,
                workspace_root: &workspace_root,
                provider: &options.provider,
                model: &options.model,
            })?;
        }
        Ok(result)
    }

    pub async fn interrupt_headless(&self, workbuddy_session_id: &str) -> bool {
        let active = self
            .active_execs
            .lock()
            .await
            .get(workbuddy_session_id)
            .cloned();
        if let Some(active) = active {
            active.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
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
        if self
            .active_turns
            .lock()
            .await
            .contains_key(workbuddy_session_id)
        {
            return Err(CodexRuntimeStateError::TurnAlreadyActive);
        }
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
        self.approval_preferences.lock().await.insert(
            thread_id.clone(),
            ApprovalPreferences {
                confirm_shell_commands: options.confirm_shell_commands,
                confirm_file_writes: options.confirm_file_writes,
            },
        );
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
        self.active_turns.lock().await.insert(
            workbuddy_session_id.to_string(),
            ActiveCodexTurn {
                thread_id: thread_id.clone(),
                turn_id: turn_id.clone(),
            },
        );
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

    pub async fn interrupt_session_turn(
        &self,
        workbuddy_session_id: &str,
    ) -> Result<(), CodexRuntimeStateError> {
        let active = self
            .active_turns
            .lock()
            .await
            .get(workbuddy_session_id)
            .cloned();
        if let Some(active) = active {
            self.interrupt_turn(&active.thread_id, &active.turn_id)
                .await?;
        }
        Ok(())
    }

    async fn finish_session_turn(&self, workbuddy_session_id: &str) {
        self.active_turns.lock().await.remove(workbuddy_session_id);
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
                            let preferences = state
                                .approval_preferences
                                .lock()
                                .await
                                .get(&thread_id)
                                .copied()
                                .unwrap_or(ApprovalPreferences {
                                    confirm_shell_commands: true,
                                    confirm_file_writes: true,
                                });
                            if should_auto_approve(&request.method, preferences) {
                                if let Err(error) = process
                                    .client()
                                    .respond(request.id, approval_result(true, false))
                                    .await
                                {
                                    tracing::warn!(
                                        "Failed to auto-approve Codex request: {}",
                                        error
                                    );
                                }
                                continue;
                            }
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
            self.approval_preferences.lock().await.remove(thread_id);
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
        self.active_turns.lock().await.clear();
        let process = self.process.lock().await.take();
        if let Some(process) = process {
            process.shutdown().await?;
        }
        Ok(())
    }
}

fn should_auto_approve(method: &str, preferences: ApprovalPreferences) -> bool {
    match method {
        "item/commandExecution/requestApproval" => !preferences.confirm_shell_commands,
        "item/fileChange/requestApproval" => !preferences.confirm_file_writes,
        _ => false,
    }
}

fn rpc_id_key(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn resolve_workbuddy_mcp_host() -> PathBuf {
    if let Some(path) = std::env::var_os("XIAOHENG_WORKBUDDY_MCP_HOST").map(PathBuf::from) {
        return path;
    }
    let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let candidates = workbuddy_mcp_host_candidates_from(&current_exe);
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .or_else(|| candidates.into_iter().next())
        .unwrap_or_else(|| PathBuf::from(workbuddy_mcp_host_binary_name(false)))
}

fn workbuddy_mcp_host_binary_name(windows: bool) -> &'static str {
    if windows {
        "workbuddy-mcp-host.exe"
    } else {
        "workbuddy-mcp-host"
    }
}

fn workbuddy_mcp_host_candidates_from(current_exe: &Path) -> Vec<PathBuf> {
    let windows = current_exe
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"));
    let binary = workbuddy_mcp_host_binary_name(windows);
    let directory = current_exe.parent().unwrap_or_else(|| Path::new("."));
    let mut candidates = vec![
        directory.join(binary),
        directory.join("workbuddy-mcp-host").join(binary),
        directory
            .join("resources")
            .join("workbuddy-mcp-host")
            .join(binary),
    ];
    if directory.file_name().and_then(|value| value.to_str()) == Some("deps") {
        if let Some(target_directory) = directory.parent() {
            candidates.push(target_directory.join(binary));
        }
    }
    if let Some(contents) = directory
        .parent()
        .filter(|path| path.file_name().and_then(|value| value.to_str()) == Some("Contents"))
    {
        candidates.push(
            contents
                .join("Resources")
                .join("resources")
                .join("workbuddy-mcp-host")
                .join(binary),
        );
        candidates.push(
            contents
                .join("Resources")
                .join("workbuddy-mcp-host")
                .join(binary),
        );
    }
    candidates
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

pub(crate) fn permission_mode_from_settings(
    confirm_shell_commands: bool,
    confirm_file_writes: bool,
) -> CodexPermissionMode {
    if confirm_shell_commands || confirm_file_writes {
        CodexPermissionMode::Approval
    } else {
        CodexPermissionMode::FullAccess
    }
}

fn parse_model_override(
    current_provider: &str,
    current_model: &str,
    value: Option<&str>,
) -> ModelOverride {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return ModelOverride::Current {
            model: current_model.to_string(),
        };
    };
    let Some((provider, model)) = value.split_once("::") else {
        return ModelOverride::Current {
            model: value.to_string(),
        };
    };
    let provider = provider.trim();
    let model = model.trim().to_string();
    if provider == "current" || provider == current_provider {
        ModelOverride::Current { model }
    } else if let Some(provider_id) = provider.strip_prefix("named:") {
        ModelOverride::Named {
            provider_id: provider_id.trim().to_string(),
            model,
        }
    } else {
        ModelOverride::Explicit {
            provider: provider.to_string(),
            model,
        }
    }
}

pub(crate) fn resolve_model_selection(
    settings: &crate::store::Settings,
    provider: Option<String>,
    model: Option<String>,
    model_override: Option<&str>,
) -> Result<(String, String), String> {
    let current_provider = provider
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| settings.provider.clone());
    let current_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| settings.model.clone());
    let selection = parse_model_override(&current_provider, &current_model, model_override);
    let (provider, model) = match selection {
        ModelOverride::Current { model } => (current_provider, model),
        ModelOverride::Explicit { provider, model } => (provider, model),
        ModelOverride::Named { provider_id, model } => {
            let named = settings
                .find_llm_provider(&provider_id)
                .ok_or_else(|| format!("找不到已启用的模型提供商配置：{provider_id}"))?;
            (named.provider.clone(), model)
        }
    };
    match provider.as_str() {
        "openai" | "deepseek" | "harmony" => Ok((provider, model)),
        _ => Err(format!(
            "当前 Codex Responses runtime 尚未配置提供商“{provider}”；请切换到 OpenAI、DeepSeek 或 Harmony Responses API"
        )),
    }
}

fn safe_attachment_name(filename: Option<&str>, media_type: &str) -> String {
    let supplied = filename
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty());
    supplied
        .unwrap_or(match media_type {
            "image/png" => "attachment.png",
            "image/jpeg" => "attachment.jpg",
            "application/pdf" => "attachment.pdf",
            _ => "attachment.bin",
        })
        .to_string()
}

fn decode_attachment_data(value: &str) -> Result<Vec<u8>, String> {
    let encoded = value
        .split_once(',')
        .filter(|(prefix, _)| prefix.starts_with("data:"))
        .map(|(_, encoded)| encoded)
        .unwrap_or(value);
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("附件 Base64 解码失败：{error}"))
}

fn materialize_chat_attachments(
    workspace_root: &str,
    session_id: &str,
    attachment: Option<LegacyChatAttachment>,
    attachments: Option<Vec<LegacyChatAttachment>>,
) -> Result<Vec<CodexAttachment>, String> {
    let source = attachments
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| attachment.into_iter().collect());
    let mut resolved = Vec::with_capacity(source.len());
    for item in source {
        let name = safe_attachment_name(item.filename.as_deref(), &item.media_type);
        let path = stage_attachment_in_workspace(
            workspace_root,
            session_id,
            &name,
            item.path.as_deref(),
            item.data.as_deref(),
        )?;
        resolved.push(CodexAttachment {
            path: path.display().to_string(),
            name,
            media_type: item.media_type,
        });
    }
    Ok(resolved)
}

fn stage_attachment_in_workspace(
    workspace_root: &str,
    session_id: &str,
    name: &str,
    source_path: Option<&str>,
    data: Option<&str>,
) -> Result<PathBuf, String> {
    let workspace = PathBuf::from(workspace_root);
    std::fs::create_dir_all(&workspace).map_err(|error| error.to_string())?;
    let workspace = workspace
        .canonicalize()
        .map_err(|error| format!("无法准备工作区：{error}"))?;

    if let Some(source_path) = source_path.filter(|path| !path.trim().is_empty()) {
        let source = PathBuf::from(source_path)
            .canonicalize()
            .map_err(|error| format!("无法读取已拖入的附件“{name}”：{error}"))?;
        if !source.is_file() {
            return Err(format!("已拖入的附件“{name}”不是可读取的文件"));
        }
        if source.starts_with(&workspace) {
            return Ok(source);
        }
        let directory = attachment_staging_directory(&workspace, session_id);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let destination = directory.join(format!("{}-{name}", uuid::Uuid::new_v4()));
        std::fs::copy(&source, &destination)
            .map_err(|error| format!("无法复制已拖入的附件“{name}”到工作区：{error}"))?;
        return Ok(destination);
    }

    if let Some(data) = data {
        let directory = attachment_staging_directory(&workspace, session_id);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(format!("{}-{name}", uuid::Uuid::new_v4()));
        std::fs::write(&path, decode_attachment_data(data)?)
            .map_err(|error| format!("附件写入失败：{error}"))?;
        return Ok(path);
    }

    Err(format!("附件“{name}”缺少本地路径或文件数据"))
}

fn attachment_staging_directory(workspace: &Path, session_id: &str) -> PathBuf {
    let safe_session = session_id
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() || value == '-' || value == '_' {
                value
            } else {
                '_'
            }
        })
        .collect::<String>();
    workspace
        .join(".workbuddy")
        .join("attachments")
        .join(safe_session)
}

const MAX_MESSAGE_CONTEXT_BYTES: usize = 16 * 1024;
const MAX_MESSAGE_CONTEXT_ITEMS: usize = 50;
const MAX_REASONING_SUMMARY_BYTES: usize = 12 * 1024;
const MAX_EXECUTION_PAYLOAD_BYTES: usize = 16 * 1024;

fn validated_message_context(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() || raw.len() > MAX_MESSAGE_CONTEXT_BYTES {
        return None;
    }
    let value = serde_json::from_str::<Value>(raw).ok()?;
    let items = match &value {
        Value::Array(items) => items,
        Value::Object(object) => object.get("annotations")?.as_array()?,
        _ => return None,
    };
    if items.len() > MAX_MESSAGE_CONTEXT_ITEMS
        || !items.iter().all(|item| item.is_object() && valid_context_value(item, 0))
    {
        return None;
    }
    serde_json::to_string(&value)
        .ok()
        .filter(|serialized| serialized.len() <= MAX_MESSAGE_CONTEXT_BYTES)
}

fn valid_context_value(value: &Value, depth: usize) -> bool {
    if depth > 8 {
        return false;
    }
    match value {
        Value::String(text) => text.len() <= 4 * 1024,
        Value::Array(items) => {
            items.len() <= MAX_MESSAGE_CONTEXT_ITEMS
                && items
                    .iter()
                    .all(|item| valid_context_value(item, depth + 1))
        }
        Value::Object(object) => {
            object.len() <= 64
                && object
                    .iter()
                    .all(|(key, value)| key.len() <= 128 && valid_context_value(value, depth + 1))
        }
        _ => true,
    }
}

fn append_reasoning_summary(target: &mut String, delta: &str) {
    if target.len() >= MAX_REASONING_SUMMARY_BYTES {
        return;
    }
    let remaining = MAX_REASONING_SUMMARY_BYTES - target.len();
    let mut boundary = remaining.min(delta.len());
    while boundary > 0 && !delta.is_char_boundary(boundary) {
        boundary -= 1;
    }
    target.push_str(&delta[..boundary]);
}

fn persisted_execution_event_type(event: &WorkBuddyEvent) -> Option<&'static str> {
    match event {
        WorkBuddyEvent::ToolStart { .. } => Some("tool_start"),
        WorkBuddyEvent::ToolEnd { .. } => Some("tool_end"),
        WorkBuddyEvent::PermissionRequest { .. } => Some("permission_request"),
        WorkBuddyEvent::PlanUpdate { .. } => Some("plan_update"),
        WorkBuddyEvent::Done { .. } => Some("done"),
        WorkBuddyEvent::Cancelled => Some("cancelled"),
        WorkBuddyEvent::Error { .. } => Some("error"),
        _ => None,
    }
}

async fn persist_execution_payload(
    app_state: &AppState,
    session_id: &str,
    turn_id: &str,
    sequence: &mut i64,
    event_type: &str,
    payload: Value,
) {
    let Ok(mut payload_json) = serde_json::to_string(&payload) else {
        return;
    };
    if payload_json.len() > MAX_EXECUTION_PAYLOAD_BYTES {
        payload_json = serde_json::json!({
            "type": event_type,
            "id": payload.get("id").cloned().unwrap_or(Value::Null),
            "name": payload.get("name").cloned().unwrap_or(Value::Null),
            "is_error": payload.get("is_error").cloned().unwrap_or(Value::Null),
            "truncated": true,
        })
        .to_string();
    }
    let result = app_state.db.lock().await.append_turn_execution_event(
        session_id,
        turn_id,
        *sequence,
        event_type,
        &payload_json,
    );
    if let Err(error) = result {
        tracing::warn!(%error, %session_id, %turn_id, %event_type, "Failed to persist Codex execution event");
        return;
    }
    *sequence += 1;
}

async fn flush_reasoning_summaries(
    app_state: &AppState,
    session_id: &str,
    turn_id: &str,
    sequence: &mut i64,
    summaries: &mut BTreeMap<(String, u32), String>,
) {
    for ((id, summary_index), text) in std::mem::take(summaries) {
        if text.trim().is_empty() {
            continue;
        }
        persist_execution_payload(
            app_state,
            session_id,
            turn_id,
            sequence,
            "reasoning_summary",
            serde_json::json!({
                "type": "reasoning_summary",
                "id": id,
                "summary_index": summary_index,
                "text": text,
            }),
        )
        .await;
    }
}

fn compose_turn_content(
    content: String,
    explicit_skills: Option<Vec<String>>,
    user_context: Option<String>,
    attachments: &[CodexAttachment],
) -> String {
    let mut sections = Vec::new();
    if let Some(context) = user_context.filter(|value| !value.trim().is_empty()) {
        sections.push(format!(
            "<workbuddy_user_context>\n{context}\n</workbuddy_user_context>"
        ));
    }
    if let Some(skills) = explicit_skills.filter(|items| !items.is_empty()) {
        sections.push(format!(
            "<workbuddy_explicit_skills>\n{}\n</workbuddy_explicit_skills>",
            skills.join("\n")
        ));
    }
    if !attachments.is_empty() {
        let files = attachments
            .iter()
            .map(|attachment| format!("- {}: {}", attachment.name, attachment.path))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "<workbuddy_attachments>\n用户已明确附加以下文件。请直接读取这些文件完成任务，不要要求用户再次提供路径。\n{files}\n</workbuddy_attachments>"
        ));
    }
    sections.push(content);
    sections.join("\n\n")
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
    codex_state: tauri::State<'_, CodexRuntimeState>,
    app_state: tauri::State<'_, AppState>,
    session_id: String,
    content: String,
    workspace_root: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    permission_mode: Option<String>,
    attachment: Option<LegacyChatAttachment>,
    attachments: Option<Vec<LegacyChatAttachment>>,
    explicit_skills: Option<Vec<String>>,
    persona_koi_id: Option<String>,
    model_override: Option<String>,
    user_context: Option<String>,
    clear_plan: Option<bool>,
) -> Result<CodexTurnStarted, String> {
    let settings = app_state.settings.lock().await.clone();
    codex_state.configure_credentials(&settings).await;
    let session_workspace = {
        let db = app_state.db.lock().await;
        db.get_session(&session_id)
            .map_err(|error| error.to_string())?
            .and_then(|session| session.workspace_root)
    };
    let workspace_root = workspace_root
        .filter(|value| !value.trim().is_empty())
        .or_else(|| session_workspace.filter(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| settings.workspace_root.clone());
    let (provider, model) =
        resolve_model_selection(&settings, provider, model, model_override.as_deref())?;
    let permission_mode = permission_mode
        .as_deref()
        .map(parse_permission_mode)
        .unwrap_or_else(|| {
            permission_mode_from_settings(
                settings.confirm_shell_commands,
                settings.confirm_file_writes,
            )
        });
    let attachments =
        materialize_chat_attachments(&workspace_root, &session_id, attachment, attachments)?;
    let visible_content = content.clone();
    let message_context_json = validated_message_context(user_context.as_deref());
    let content = compose_turn_content(content, explicit_skills, user_context, &attachments);
    let mut developer_instructions =
        "你是小亨，运行于 Harmony Work。始终使用名称“小亨”，作为用户的桌面 AI 工作伙伴直接完成任务。当前唯一运行时是 Codex app-server；使用 Codex 原生工具和 WorkBuddy MCP 能力。"
            .to_string();
    if let Some(persona) = persona_koi_id.filter(|value| !value.trim().is_empty()) {
        developer_instructions.push_str(&format!(
            "\n当前用户选择的人员/角色 ID 是 {persona}。在 WorkBuddy MCP 人员能力可用时读取其职责并按该角色执行。"
        ));
    }
    let _ = clear_plan;
    let options = CodexTurnOptions {
        content: content.clone(),
        workspace_root,
        provider,
        model,
        permission_mode,
        sandbox_mode: if settings.allow_outside_workspace {
            CodexSandboxMode::FullAccess
        } else {
            CodexSandboxMode::WorkspaceWrite
        },
        confirm_shell_commands: settings.confirm_shell_commands,
        confirm_file_writes: settings.confirm_file_writes,
        developer_instructions: Some(developer_instructions),
        attachments,
    };
    {
        let db = app_state.db.lock().await;
        db.append_message_with_context(
            &session_id,
            "user",
            &visible_content,
            message_context_json.as_deref(),
        )
            .map_err(|error| error.to_string())?;
        db.update_session_status(&session_id, "running")
            .map_err(|error| error.to_string())?;
    }
    let execution = match codex_state.start_turn(&session_id, options).await {
        Ok(execution) => execution,
        Err(error) => {
            let db = app_state.db.lock().await;
            let _ = db.update_session_status(&session_id, "idle");
            return Err(error.to_string());
        }
    };
    let started = CodexTurnStarted {
        thread_id: execution.thread_id.clone(),
        turn_id: execution.turn_id.clone(),
    };
    let history_turn_id = execution.turn_id.clone();
    let event_name = format!("agent_event_{session_id}");
    let mut events = execution.events;
    let codex_state = codex_state.inner().clone();
    let app_state = app_state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut transcript = CodexTurnTranscript::default();
        let mut execution_sequence = 1_i64;
        let mut reasoning_summaries = BTreeMap::<(String, u32), String>::new();
        persist_execution_payload(
            &app_state,
            &session_id,
            &history_turn_id,
            &mut execution_sequence,
            "turn_started",
            serde_json::json!({"type":"turn_started"}),
        )
        .await;
        while let Some(event) = events.recv().await {
            transcript.observe(&event);
            match &event {
                WorkBuddyEvent::ReasoningSummaryStart { id, summary_index } => {
                    reasoning_summaries
                        .entry((id.clone(), *summary_index))
                        .or_default();
                }
                WorkBuddyEvent::ReasoningSummaryDelta {
                    id,
                    summary_index,
                    delta,
                } => append_reasoning_summary(
                    reasoning_summaries
                        .entry((id.clone(), *summary_index))
                        .or_default(),
                    delta,
                ),
                _ => {}
            }
            if matches!(
                &event,
                WorkBuddyEvent::ToolStart { .. }
                    | WorkBuddyEvent::PermissionRequest { .. }
                    | WorkBuddyEvent::PlanUpdate { .. }
                    | WorkBuddyEvent::Done { .. }
                    | WorkBuddyEvent::Cancelled
                    | WorkBuddyEvent::Error { .. }
            ) {
                flush_reasoning_summaries(
                    &app_state,
                    &session_id,
                    &history_turn_id,
                    &mut execution_sequence,
                    &mut reasoning_summaries,
                )
                .await;
            }
            if let Some(event_type) = persisted_execution_event_type(&event) {
                if let Ok(payload) = serde_json::to_value(&event) {
                    persist_execution_payload(
                        &app_state,
                        &session_id,
                        &history_turn_id,
                        &mut execution_sequence,
                        event_type,
                        payload,
                    )
                    .await;
                }
            }
            if let WorkBuddyEvent::ToolEnd {
                id,
                name,
                result,
                is_error: false,
            } = &event
            {
                for candidate in extract_artifact_candidates(result) {
                    let artifact = {
                        let db = app_state.db.lock().await;
                        db.add_session_artifact(
                            &session_id,
                            &candidate.name,
                            &candidate.artifact_type,
                            Some(candidate.uri.as_str()),
                            &candidate.summary,
                            Some("workbuddy_mcp"),
                            Some(id),
                            candidate.metadata_json.as_deref(),
                        )
                        .ok()
                    };
                    if let Some(artifact) = artifact {
                        let _ =
                            app.emit(&format!("session_artifacts_updated_{session_id}"), artifact);
                    }
                }
                match name.as_str() {
                    "skill_install" | "skill_set_enabled" | "skill_uninstall" => {
                        let _ = app.emit("skills_changed", ());
                    }
                    "staff_status_set" => {
                        let _ = app.emit("koi_status_changed", serde_json::json!({}));
                        let _ = app.emit("koi_updated", serde_json::json!({}));
                    }
                    "staff_task_create" => {
                        let _ = app.emit("koi_todo_updated", serde_json::json!({}));
                        if let Some(todo_id) = extract_staff_task_id(result) {
                            let task_app = app.clone();
                            let task_state = app_state.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(error) = crate::pool::bridge::dispatch_todo_by_id(
                                    &task_app,
                                    &task_state,
                                    &todo_id,
                                )
                                .await
                                {
                                    tracing::error!(%error, %todo_id, "Codex StaffDeck dispatch failed");
                                }
                            });
                        }
                    }
                    "staff_task_update" => {
                        let _ = app.emit("koi_todo_updated", serde_json::json!({}));
                    }
                    _ => {}
                }
            }
            let terminal = is_terminal_event(&event);
            if terminal {
                {
                    let db = app_state.db.lock().await;
                    if matches!(&event, WorkBuddyEvent::Done { .. }) {
                        let tool_calls_json = transcript.tool_calls_json();
                        let tool_results_json = transcript.tool_results_json();
                        if let Some(tool_calls_json) = tool_calls_json.as_deref() {
                            let _ = db.append_message_full(
                                &session_id,
                                "assistant",
                                "",
                                Some(tool_calls_json),
                                None,
                                None,
                            );
                        }
                        if let Some(tool_results_json) = tool_results_json.as_deref() {
                            let _ = db.append_message_full(
                                &session_id,
                                "user",
                                "",
                                None,
                                Some(tool_results_json),
                                None,
                            );
                        }
                        if !transcript.assistant_text.trim().is_empty() {
                            let _ = db.append_message(
                                &session_id,
                                "assistant",
                                &transcript.assistant_text,
                            );
                        }
                    }
                    let _ = db.update_session_status(&session_id, "idle");
                }
                codex_state.finish_session_turn(&session_id).await;
                let _ = app.emit(&event_name, event);
                break;
            }
            let _ = app.emit(&event_name, event);
        }
    });
    Ok(started)
}

#[tauri::command]
pub async fn codex_chat_cancel(
    state: tauri::State<'_, CodexRuntimeState>,
    session_id: Option<String>,
    thread_id: Option<String>,
    turn_id: Option<String>,
) -> Result<(), String> {
    if let Some(session_id) = session_id {
        return state
            .interrupt_session_turn(&session_id)
            .await
            .map_err(|error| error.to_string());
    }
    match (thread_id, turn_id) {
        (Some(thread_id), Some(turn_id)) => state
            .interrupt_turn(&thread_id, &turn_id)
            .await
            .map_err(|error| error.to_string()),
        _ => Err("缺少 sessionId 或 threadId/turnId".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_transcript_projects_tools_and_plan_into_legacy_message_blocks() {
        let mut transcript = CodexTurnTranscript::default();
        transcript.observe(&WorkBuddyEvent::ToolStart {
            id: "tool-1".to_string(),
            name: "shell".to_string(),
            input: serde_json::json!({"command":"pwd"}),
        });
        transcript.observe(&WorkBuddyEvent::ToolEnd {
            id: "tool-1".to_string(),
            name: "shell".to_string(),
            result: "/work".to_string(),
            is_error: false,
        });
        transcript.observe(&WorkBuddyEvent::PlanUpdate {
            items: vec![WorkBuddyPlanItem {
                id: "step-1".to_string(),
                content: "检查环境".to_string(),
                status: "completed".to_string(),
            }],
        });
        transcript.observe(&WorkBuddyEvent::TextDelta {
            delta: "已完成".to_string(),
        });

        let calls = transcript.tool_calls_json().expect("tool calls");
        let results = transcript.tool_results_json().expect("tool results");
        assert!(calls.contains("shell"));
        assert!(calls.contains("plan_todo"));
        assert!(results.contains("/work"));
        assert_eq!(transcript.assistant_text, "已完成");
    }

    #[test]
    fn extracts_office_and_harvis_artifacts_from_codex_mcp_results() {
        let result = serde_json::json!({
            "type":"mcpToolCall",
            "result":{
                "structuredContent":{
                    "artifacts":[
                        {"name":"report.docx","artifactType":"document","uri":"C:/Harmony/report.docx"},
                        {"fileName":"poster.png","mediaType":"image/png","localPath":"C:/Harmony/marketing-assets/poster.png"}
                    ]
                },
                "content":[
                    {"type":"resource_link","name":"report.docx","uri":"C:/Harmony/report.docx","mimeType":"application/vnd.openxmlformats-officedocument.wordprocessingml.document"}
                ]
            }
        })
        .to_string();

        let artifacts = extract_artifact_candidates(&result);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].name, "report.docx");
        assert_eq!(artifacts[0].artifact_type, "document");
        assert_eq!(artifacts[1].name, "poster.png");
        assert_eq!(artifacts[1].artifact_type, "image");
    }

    #[test]
    fn extracts_created_staff_task_id_from_structured_mcp_result() {
        let result = serde_json::json!({
            "result": {
                "structuredContent": {
                    "id": "todo-42",
                    "ownerId": "staff-1",
                    "title": "生成职责介绍",
                    "status": "todo"
                }
            }
        })
        .to_string();

        assert_eq!(extract_staff_task_id(&result).as_deref(), Some("todo-42"));
    }

    #[test]
    fn legacy_model_override_keeps_or_replaces_provider_explicitly() {
        assert_eq!(
            parse_model_override("deepseek", "deepseek-v4-pro", Some("deepseek-chat")),
            ModelOverride::Current {
                model: "deepseek-chat".to_string(),
            }
        );
        assert_eq!(
            parse_model_override("deepseek", "deepseek-v4-pro", Some("openai::gpt-5.4")),
            ModelOverride::Explicit {
                provider: "openai".to_string(),
                model: "gpt-5.4".to_string(),
            }
        );
        assert_eq!(
            parse_model_override(
                "deepseek",
                "deepseek-v4-pro",
                Some("named:team-provider::deepseek-reasoner"),
            ),
            ModelOverride::Named {
                provider_id: "team-provider".to_string(),
                model: "deepseek-reasoner".to_string(),
            }
        );
    }

    #[test]
    fn confirmation_flags_map_to_codex_permission_mode() {
        assert_eq!(
            permission_mode_from_settings(true, false),
            CodexPermissionMode::Approval
        );
        assert_eq!(
            permission_mode_from_settings(false, false),
            CodexPermissionMode::FullAccess
        );
    }

    #[test]
    fn shell_confirmation_can_be_disabled_while_file_changes_still_require_review() {
        let preferences = ApprovalPreferences {
            confirm_shell_commands: false,
            confirm_file_writes: true,
        };
        assert!(should_auto_approve(
            "item/commandExecution/requestApproval",
            preferences
        ));
        assert!(!should_auto_approve(
            "item/fileChange/requestApproval",
            preferences
        ));
    }

    #[test]
    fn turn_prompt_explicitly_identifies_staged_attachments() {
        let content = compose_turn_content(
            "请分析附件".to_string(),
            None,
            None,
            &[CodexAttachment {
                path: "/workspace/.workbuddy/attachments/session/report.csv".to_string(),
                name: "report.csv".to_string(),
                media_type: "text/csv".to_string(),
            }],
        );
        assert!(content.contains("<workbuddy_attachments>"));
        assert!(content.contains("report.csv"));
        assert!(content.contains("/workspace/.workbuddy/attachments/session/report.csv"));
    }

    #[test]
    fn annotation_context_is_validated_separately_from_visible_message_text() {
        let raw = r#"[{"targetType":"artifact","artifactId":"report-1","comment":"标题更简洁"}]"#;
        let context = validated_message_context(Some(raw)).expect("valid annotation context");
        let prompt = compose_turn_content(
            "请按批注修改".to_string(),
            None,
            Some(context.clone()),
            &[],
        );

        assert_eq!(
            serde_json::from_str::<Value>(&context).expect("normalized context"),
            serde_json::from_str::<Value>(raw).expect("source context")
        );
        assert!(prompt.contains("<workbuddy_user_context>"));
        assert!(prompt.ends_with("请按批注修改"));
        assert!(validated_message_context(Some("登录用户：测试")).is_none());
        assert!(validated_message_context(Some(r#"["not-an-object"]"#)).is_none());
    }

    #[test]
    fn external_attachment_is_copied_into_workspace_before_codex_receives_it() {
        let root =
            std::env::temp_dir().join(format!("workbuddy-attachment-{}", uuid::Uuid::new_v4()));
        let external =
            std::env::temp_dir().join(format!("workbuddy-external-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&external, "attachment content").expect("write source");

        let staged = stage_attachment_in_workspace(
            &root.display().to_string(),
            "session-1",
            "example.txt",
            Some(&external.display().to_string()),
            None,
        )
        .expect("stage attachment");

        assert!(staged.starts_with(
            root.canonicalize()
                .expect("workspace exists")
                .join(".workbuddy")
                .join("attachments")
        ));
        assert_eq!(
            std::fs::read_to_string(&staged).expect("read staged"),
            "attachment content"
        );
        let _ = std::fs::remove_file(external);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_mcp_host_candidates_cover_build_and_installer_layouts() {
        let candidates = workbuddy_mcp_host_candidates_from(Path::new(
            "C:/Program Files/Harmony Work/harmony-work.exe",
        ));
        assert!(candidates.iter().any(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .ends_with("resources/workbuddy-mcp-host/workbuddy-mcp-host.exe")
        }));
        assert!(candidates.iter().any(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .ends_with("Harmony Work/workbuddy-mcp-host.exe")
        }));
        let legacy_candidates = workbuddy_mcp_host_candidates_from(Path::new(
            "C:/Program Files/Xiaoheng/xiaoheng-workbuddy.exe",
        ));
        assert!(legacy_candidates.iter().any(|path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .ends_with("Xiaoheng/workbuddy-mcp-host.exe")
        }));
    }

    #[tokio::test]
    async fn reports_stopped_runtime_without_launching_process() {
        let directory = std::env::temp_dir().join(format!("codex-state-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create state dir");
        let state = CodexRuntimeState::new(&directory).expect("state");

        let diagnostic = state.diagnostic(false).await.expect("diagnostic");
        assert!(!diagnostic.running);
        assert_eq!(diagnostic.display_version, crate::codex_runtime::config::DISPLAY_VERSION);
        assert_eq!(diagnostic.target_codex_version, "0.149.0");
        assert_eq!(diagnostic.mapped_sessions, 0);

        drop(state);
        std::fs::remove_dir_all(directory).expect("remove state dir");
    }

    #[tokio::test]
    async fn interrupts_active_headless_session() {
        let directory = std::env::temp_dir().join(format!("codex-cancel-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create state dir");
        let state = CodexRuntimeState::new(&directory).expect("state");
        let flag = Arc::new(AtomicBool::new(false));
        state
            .active_execs
            .lock()
            .await
            .insert("session-1".to_string(), flag.clone());

        assert!(state.interrupt_headless("session-1").await);
        assert!(flag.load(Ordering::Relaxed));
        assert!(!state.interrupt_headless("missing").await);

        drop(state);
        std::fs::remove_dir_all(directory).expect("remove state dir");
    }
}
