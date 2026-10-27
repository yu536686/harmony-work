use super::*;
use serde_json::json;

#[test]
fn runtime_regression_parent_usage_commands_have_a_valid_tauri_handler_contract() {
    let _handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        crate::commands::usage_policy::usage_get,
        crate::commands::usage_policy::usage_set_budget,
    ];
}

#[test]
fn runtime_regression_attachment_history_round_trips_with_annotations() {
    let root = std::env::temp_dir().join(format!("runtime-history-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("history.db");
    let attachments = vec![
        CodexAttachment {
            path: "/work/a.png".into(),
            name: "a.png".into(),
            media_type: "image/png".into(),
        },
        CodexAttachment {
            path: "/work/b.pdf".into(),
            name: "b.pdf".into(),
            media_type: "application/pdf".into(),
        },
        CodexAttachment {
            path: "/work/c.jpg".into(),
            name: "c.jpg".into(),
            media_type: "image/jpeg".into(),
        },
    ];
    let annotations = json!([{"comment":"keep annotation"}]);
    let context = persisted_message_context(None, Some(annotations.to_string()), &attachments);
    let session_id = {
        let db = crate::store::Database::open(&path).unwrap();
        let session = db.create_session(None).unwrap();
        db.append_message_with_context(&session.id, "user", "", context.as_deref())
            .unwrap();
        session.id
    };
    let db = crate::store::Database::open(&path).unwrap();
    let message = db.get_messages_latest(&session_id, 1).unwrap().remove(0);
    let value: Value = serde_json::from_str(message.context_json.as_deref().unwrap()).unwrap();
    assert_eq!(
        value["attachments"],
        serde_json::to_value(&attachments).unwrap()
    );
    assert_eq!(value["annotations"], annotations);
    assert_eq!(message.content, "");
    drop(db);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_regression_artifact_dedup_does_not_hide_another_sessions_path() {
    let db = crate::store::Database::open_in_memory().unwrap();
    let a = db.create_session(None).unwrap();
    let b = db.create_session(None).unwrap();
    db.add_session_artifact(
        &a.id,
        "report.pdf",
        "pdf",
        Some("/work/report.pdf"),
        "",
        None,
        None,
        None,
    )
    .unwrap();
    let path = normalize_artifact_uri("/work/report.pdf");
    assert!(known_session_artifact_uris(&db, &a.id).contains(&path));
    assert!(!known_session_artifact_uris(&db, &b.id).contains(&path));
    db.add_session_artifact(
        &b.id,
        "report.pdf",
        "pdf",
        Some("/work/report.pdf"),
        "",
        None,
        None,
        None,
    )
    .unwrap();
    assert!(known_session_artifact_uris(&db, &b.id).contains(&path));
    assert_eq!(db.list_session_artifacts(&a.id, 10).unwrap().len(), 1);
    assert_eq!(db.list_session_artifacts(&b.id, 10).unwrap().len(), 1);
}

#[test]
fn runtime_regression_message_context_preserves_legacy_annotations_and_shared_import() {
    let annotations = json!([{"comment":"keep"}]).to_string();
    assert_eq!(
        persisted_message_context(None, Some(annotations.clone()), &[]),
        Some(annotations.clone())
    );
    assert_eq!(persisted_message_context(None, None, &[]), None);
    let attachments = vec![CodexAttachment {
        path: "/work/a.png".into(),
        name: "a.png".into(),
        media_type: "image/png".into(),
    }];
    let context = persisted_message_context(
        Some(json!({"annotations":[{"comment":"legacy"}]}).to_string()),
        Some(annotations),
        &attachments,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&context).unwrap();
    assert_eq!(value["userContext"]["annotations"][0]["comment"], "legacy");
    assert_eq!(value["annotations"][0]["comment"], "keep");
    let db = crate::store::Database::open_in_memory().unwrap();
    let session = db.create_session(None).unwrap();
    db.append_message(&session.id, "user", "look at this")
        .unwrap();
    persist_user_message(&db, &session.id, "look at this", Some(&context), true).unwrap();
    let messages = db.get_messages_latest(&session.id, 10).unwrap();
    assert_eq!(
        messages.len(),
        1,
        "shared import must not duplicate the user message"
    );
    assert_eq!(messages[0].context_json.as_deref(), Some(context.as_str()));
}

#[test]
fn runtime_regression_named_selection_keeps_configuration_identity() {
    let mut settings = crate::store::Settings::default();
    settings
        .llm_providers
        .push(crate::store::settings::LlmProviderConfig {
            id: "team".into(),
            provider: "openai".into(),
            api_key: "team-secret".into(),
            base_url: "https://team.example/v1".into(),
            ..Default::default()
        });
    let (provider, model) =
        resolve_model_selection(&settings, None, None, Some("named:team::model-x")).unwrap();
    assert_eq!(provider, "workbuddy_named_7465616d");
    assert_eq!(model, "model-x");
}

#[tokio::test]
async fn runtime_regression_old_approval_timeout_cannot_remove_replacement() {
    let root = std::env::temp_dir().join(format!("runtime-approval-{}", uuid::Uuid::new_v4()));
    let state = CodexRuntimeState::new(&root).unwrap();
    let old = TurnRoute {
        process_id: 1,
        generation: Arc::new(AtomicBool::new(false)),
    };
    let new = TurnRoute {
        process_id: 1,
        generation: Arc::new(AtomicBool::new(false)),
    };
    state.pending_approvals.lock().await.insert(
        "reused".into(),
        PendingApproval {
            rpc_id: json!("reused"),
            kind: PendingApprovalKind::Standard,
            thread_id: "thread".into(),
            route: new.clone(),
        },
    );
    assert!(state.take_owned_approval("reused", &old).await.is_none());
    assert!(state.take_owned_approval("reused", &new).await.is_some());
    drop(state);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn runtime_regression_custom_responses_is_supported_but_messages_protocol_is_not() {
    let mut settings = crate::store::Settings::default();
    settings.provider = "custom".into();
    settings.custom_base_url = "https://responses.example/v1".into();
    assert!(resolve_model_selection(&settings, None, None, None).is_ok());
    settings.provider = "anthropic".into();
    assert!(resolve_model_selection(&settings, None, None, None)
        .unwrap_err()
        .contains("Responses"));
}

#[test]
fn runtime_regression_provider_matrix_matches_the_main_ui_contract() {
    assert_eq!(
        crate::codex_runtime::config::RESPONSES_PROVIDER_IDS,
        &["openai", "harmony", "custom"]
    );
    let mut settings = crate::store::Settings::default();
    for unsupported in [
        "deepseek",
        "anthropic",
        "qwen",
        "kimi",
        "siliconflow",
        "minimax",
        "zhipu",
        "ollama",
    ] {
        settings.provider = unsupported.into();
        assert!(resolve_model_selection(&settings, None, None, None).is_err(), "external provider {unsupported} must not be falsely advertised as Responses-compatible");
    }
}

#[tokio::test]
async fn runtime_regression_named_credentials_and_url_are_not_global_fallbacks() {
    let root = std::env::temp_dir().join(format!("runtime-provider-{}", uuid::Uuid::new_v4()));
    let state = CodexRuntimeState::new(&root).unwrap();
    let mut settings = crate::store::Settings::default();
    settings.openai_api_key = "global-secret".into();
    settings
        .llm_providers
        .push(crate::store::settings::LlmProviderConfig {
            id: "team".into(),
            provider: "openai".into(),
            api_key: "team-secret".into(),
            base_url: "https://team.example/v1".into(),
            ..Default::default()
        });
    state.configure_credentials(&settings).await;
    let env = state.environment.lock().await.clone();
    let raw = fs::read_to_string(state.codex_home.join("config.toml")).unwrap();
    let config: toml::Value = raw.parse().unwrap();
    let profile = &config["model_providers"];
    assert!(
        profile.get("openai").is_none(),
        "Codex reserves the built-in openai provider ID"
    );
    assert_eq!(
        profile
            .get("workbuddy_named_7465616d")
            .and_then(|p| p.get("base_url"))
            .and_then(toml::Value::as_str),
        Some("https://team.example/v1")
    );
    let key_var = profile["workbuddy_named_7465616d"]["env_key"]
        .as_str()
        .unwrap();
    assert_eq!(env.get(key_var).map(String::as_str), Some("team-secret"));
    assert!(!raw.contains("team-secret"));
    assert!(!raw.contains("global-secret"));
    settings.llm_providers[0].api_key.clear();
    assert!(
        resolve_model_selection(&settings, None, None, Some("named:team::test")).is_err(),
        "missing named key must not inherit global key"
    );
    drop(state);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn runtime_regression_custom_config_uses_responses_and_its_base() {
    let root = std::env::temp_dir().join(format!("runtime-custom-{}", uuid::Uuid::new_v4()));
    let state = CodexRuntimeState::new(&root).unwrap();
    let mut settings = crate::store::Settings::default();
    settings.provider = "custom".into();
    settings.openai_api_key = "custom-secret".into();
    settings.custom_base_url = "https://custom.example/api/v1".into();
    state.configure_credentials(&settings).await;
    let raw = fs::read_to_string(state.codex_home.join("config.toml")).unwrap();
    let config: toml::Value = raw.parse().unwrap();
    let custom = config["model_providers"].get("custom");
    assert_eq!(
        custom
            .and_then(|p| p.get("base_url"))
            .and_then(toml::Value::as_str),
        Some("https://custom.example/api/v1")
    );
    assert_eq!(custom.unwrap()["wire_api"].as_str(), Some("responses"));
    drop(state);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
mod cancellation {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    async fn wait_file(path: &Path) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake app-server reached barrier");
    }

    fn options(root: &Path) -> CodexTurnOptions {
        CodexTurnOptions {
            content: "test".into(),
            workspace_root: root.display().to_string(),
            provider: "openai".into(),
            model: "test".into(),
            permission_mode: CodexPermissionMode::FullAccess,
            sandbox_mode: CodexSandboxMode::WorkspaceWrite,
            confirm_shell_commands: false,
            confirm_file_writes: false,
            confirm_mcp_tool_calls: false,
            developer_instructions: None,
            attachments: vec![],
        }
    }

    fn fixture(phase: &str, response: &str) -> (CodexRuntimeState, PathBuf) {
        let root = std::env::temp_dir().join(format!("runtime-race-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let script = root.join("codex.cjs");
        let program = r#"#!/usr/bin/env node
const fs = require('fs');
const root = __dirname;
if (process.argv.includes('--version')) { console.log('codex-cli 0.156.1'); process.exit(0); }
if (process.argv[2] === 'exec') {
  process.stdin.resume();
  process.stdin.on('end', () => {
    fs.writeFileSync(root + '/headless-generated.txt', 'generated');
    for (const event of [{type:'thread.started',thread_id:'real-headless-thread'},
      {type:'item.completed',item:{type:'agent_message',text:'done'}},
      {type:'turn.completed',usage:{input_tokens:5,output_tokens:2}}]) console.log(JSON.stringify(event));
    if (PHASE === 'exec/failure') process.exitCode = 7;
    if (PHASE === 'exec/cancel') setInterval(() => {}, 1000);
    if (PHASE === 'exec/read-error') process.stdout.write(Buffer.from([0xff, 0x0a]));
  });
  return;
}
require('readline').createInterface({input:process.stdin}).on('line', async line => {
  const r = JSON.parse(line);
  fs.appendFileSync(root + '/requests', r.method + '\n');
  if (r.method === PHASE) {
    if (r.method === 'turn/start' && fs.existsSync(root + '/spawn-writer')) {
      require('child_process').spawn(process.execPath, ['-e', `const fs=require('fs'); setInterval(()=>fs.appendFileSync(${JSON.stringify(root + '/writer-ticks')}, 'x'), 5);`], {stdio:'ignore'});
    }
    fs.writeFileSync(root + '/waiting', '');
    while (!fs.existsSync(root + '/release')) await new Promise(resolve => setTimeout(resolve, 5));
    console.log(JSON.stringify({id:r.id, ...RESPONSE})); return;
  }
  if (r.id == null) return;
  if (r.method === 'test/notify') console.log(JSON.stringify(r.params));
  if (r.method === 'test/complete') {
    fs.writeFileSync(root + '/generated.txt', 'generated');
    console.log(JSON.stringify({method:'turn/completed',params:{threadId:r.params.threadId || 'thread-1',turn:{id:r.params.turnId || 'turn-1',status:'completed'}}}));
  }
  const result = r.method === 'initialize' ? {userAgent:'test'} :
    r.method.startsWith('thread/') ? {thread:{id:r.params.threadId || 'thread-1'}} :
    r.method === 'turn/start' ? {turn:{id:'turn-' + (++turnSequence)}} : {};
  console.log(JSON.stringify({id:r.id, result}));
});
"#;
        fs::write(
            &script,
            program
                .replace(
                    "require('readline')",
                    "let turnSequence = 0; require('readline')",
                )
                .replace("PHASE", &json!(phase).to_string())
                .replace("RESPONSE", response),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let mut state = CodexRuntimeState::new(&root).unwrap();
        state.executable_override = Some(script);
        (state, root)
    }

    #[tokio::test]
    async fn runtime_regression_late_interrupt_cannot_cancel_replacement() {
        let (state, root) = fixture("turn/interrupt", r#"{"result":{}}"#);
        let mut first = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        let stopping = state.clone();
        let stop = tokio::spawn(async move { stopping.interrupt_session_turn("s").await });
        wait_file(&root.join("waiting")).await;
        process.client().request("test/notify", json!({"method":"item/agentMessage/delta","params":{"threadId":first.thread_id,"turnId":first.turn_id,"delta":"old"}})).await.unwrap();
        assert!(matches!(
            first.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        state.finish_session_turn("s", &first.generation).await;
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.pending_captures.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut second = state.start_turn("s", options(&root)).await.unwrap();
        assert_ne!(first.turn_id, second.turn_id);
        fs::write(root.join("release"), "").unwrap();
        stop.await.unwrap().unwrap();
        let late = tokio::time::timeout(Duration::from_millis(150), second.events.recv()).await;
        let preserved = state
            .subscribers
            .lock()
            .await
            .contains_key(&second.thread_id);
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            late.is_err(),
            "old interrupt dispatched to replacement: {late:?}"
        );
        assert!(preserved, "old interrupt removed replacement subscriber");
    }

    #[tokio::test]
    async fn runtime_regression_old_remote_terminal_cannot_finish_replacement() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let first = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.pending_captures.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.finish_session_turn("s", &first.generation).await;
        let mut second = state.start_turn("s", options(&root)).await.unwrap();
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        let late = tokio::time::timeout(Duration::from_millis(150), second.events.recv()).await;
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            late.is_err(),
            "old terminal dispatched to replacement: {late:?}"
        );
    }

    async fn stop_without_releasing_startup(phase: &str, resume: bool) {
        let (state, root) = fixture(
            phase,
            r#"{"result":{"thread":{"id":"thread-1"},"turn":{"id":"turn-1"}}}"#,
        );
        if resume {
            state
                .sessions
                .bind(NewCodexSessionBinding {
                    workbuddy_session_id: "s",
                    codex_thread_id: "thread-1",
                    workspace_root: root.to_str().unwrap(),
                    provider: "openai",
                    model: "test",
                })
                .unwrap();
        }
        let running_state = state.clone();
        let opts = options(&root);
        let mut running = tokio::spawn(async move { running_state.start_turn("s", opts).await });
        wait_file(&root.join("waiting")).await;
        state.interrupt_session_turn("s").await.unwrap();
        let stopped = tokio::time::timeout(Duration::from_millis(500), &mut running).await;
        let bounded = stopped.is_ok();
        if !bounded {
            fs::write(root.join("release"), "").unwrap();
            let _ = running.await;
        }
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(bounded, "Stop left {phase} waiting for the server barrier");
    }

    #[tokio::test]
    async fn runtime_regression_stalled_initialize_stop_is_bounded() {
        stop_without_releasing_startup("initialize", false).await;
    }
    #[tokio::test]
    async fn runtime_regression_stalled_resume_stop_is_bounded() {
        stop_without_releasing_startup("thread/resume", true).await;
    }
    #[tokio::test]
    async fn runtime_regression_stalled_turn_start_stop_is_bounded() {
        stop_without_releasing_startup("turn/start", false).await;
    }

    #[tokio::test]
    async fn runtime_regression_uncertain_start_retains_guard_until_process_is_reaped() {
        let (state, root) = fixture("turn/start", r#"{"result":{"turn":{"id":"held"}}}"#);
        let process = state.ensure_started().await.unwrap();
        fs::write(root.join("spawn-writer"), "").unwrap();
        let running_state = state.clone();
        let opts = options(&root);
        let running = tokio::spawn(async move { running_state.start_turn("s", opts).await });
        wait_file(&root.join("waiting")).await;
        wait_file(&root.join("writer-ticks")).await;
        let shutdown_gate = process.hold_shutdown().await;
        state.interrupt_session_turn("s").await.unwrap();
        // The child mutex is an exact retirement barrier, not a timing assumption.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!running.is_finished());
        assert!(state
            .pending_captures
            .lock()
            .await
            .values()
            .any(|capture| capture.writers_may_run));
        let probe = RuntimeTurnCapture::begin(root.to_str().unwrap(), "writer-probe").await;
        assert!(
            probe.before_error.is_some(),
            "uncertain remote start released its writer guard"
        );
        drop(shutdown_gate);
        assert!(tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(!process.is_running().await.unwrap());
        let writes_after_reap = fs::metadata(root.join("writer-ticks")).unwrap().len();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            fs::metadata(root.join("writer-ticks")).unwrap().len(),
            writes_after_reap,
            "a descendant writer survived retirement"
        );
        assert!(state.pending_captures.lock().await.is_empty());
        let _ = probe.close_after_stop().await;
        let next = RuntimeTurnCapture::begin(root.to_str().unwrap(), "after-retirement").await;
        assert!(next.before_error.is_none());
        next.finish().await.unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_stalled_resume_preserves_another_active_turn() {
        let (state, root) = fixture(
            "thread/resume",
            r#"{"result":{"thread":{"id":"thread-2"}}}"#,
        );
        let mut first = state.start_turn("a", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        state
            .sessions
            .bind(NewCodexSessionBinding {
                workbuddy_session_id: "b",
                codex_thread_id: "thread-2",
                workspace_root: root.to_str().unwrap(),
                provider: "openai",
                model: "test",
            })
            .unwrap();
        let running_state = state.clone();
        let opts = options(&root);
        let running = tokio::spawn(async move { running_state.start_turn("b", opts).await });
        wait_file(&root.join("waiting")).await;
        state.interrupt_session_turn("b").await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(process.is_running().await.unwrap());
        process.client().request("test/notify", json!({"method":"item/agentMessage/delta","params":{"threadId":first.thread_id,"turnId":first.turn_id,"delta":"still live"}})).await.unwrap();
        assert!(
            matches!(first.events.recv().await, Some(WorkBuddyEvent::TextDelta { delta }) if delta == "still live")
        );
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_uncertain_start_teardown_notifies_other_turns() {
        let (state, root) = fixture("turn/start", r#"{"result":{"turn":{"id":"first"}}}"#);
        fs::write(root.join("release"), "").unwrap();
        let mut first = state.start_turn("a", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        fs::remove_file(root.join("release")).unwrap();
        fs::remove_file(root.join("waiting")).unwrap();
        state
            .sessions
            .bind(NewCodexSessionBinding {
                workbuddy_session_id: "b",
                codex_thread_id: "thread-2",
                workspace_root: root.to_str().unwrap(),
                provider: "openai",
                model: "test",
            })
            .unwrap();
        let running_state = state.clone();
        let opts = options(&root);
        let running = tokio::spawn(async move { running_state.start_turn("b", opts).await });
        wait_file(&root.join("waiting")).await;
        state.interrupt_session_turn("b").await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        let event = first.events.recv().await;
        assert!(
            matches!(event, Some(WorkBuddyEvent::Error { message }) if message.contains("stopped for safety")),
            "collateral turn must receive an honest failure"
        );
        assert!(!process.is_running().await.unwrap());
        assert!(state.pending_captures.lock().await.is_empty());
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_startup_times_out_without_stop_or_server_release() {
        for phase in ["initialize", "thread/start", "thread/resume", "turn/start"] {
            let (mut state, root) = fixture(phase, r#"{"result":{}}"#);
            if phase != "initialize" {
                state.ensure_started().await.unwrap();
            }
            if phase == "thread/resume" {
                state
                    .sessions
                    .bind(NewCodexSessionBinding {
                        workbuddy_session_id: "s",
                        codex_thread_id: "thread-1",
                        workspace_root: root.to_str().unwrap(),
                        provider: "openai",
                        model: "test",
                    })
                    .unwrap();
            }
            state.rpc_timeout = Duration::from_millis(200);
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                state.start_turn("s", options(&root)),
            )
            .await
            .expect("startup deadline");
            assert!(result.is_err(), "{phase} did not time out");
            assert!(state.active_turns.lock().await.is_empty());
            assert!(state.pending_captures.lock().await.is_empty());
            state.shutdown().await.unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[tokio::test]
    async fn runtime_regression_obsolete_cleanup_cannot_remove_replacement() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let first = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.pending_captures.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.finish_session_turn("s", &first.generation).await;
        let second = state.start_turn("s", options(&root)).await.unwrap();
        assert!(!state.finish_session_turn("s", &first.generation).await);
        assert!(state
            .subscribers
            .lock()
            .await
            .get(&second.thread_id)
            .is_some_and(|subscriber| Arc::ptr_eq(
                &subscriber.route.generation,
                &second.generation
            )));
        assert!(state
            .active_turns
            .lock()
            .await
            .get("s")
            .is_some_and(|turn| Arc::ptr_eq(&turn.cancellation, &second.generation)));
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_snapshot_wait_does_not_drop_known_usage() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let db = Arc::new(Mutex::new(
            crate::store::Database::open_in_memory().unwrap(),
        ));
        db.lock()
            .await
            .ensure_fixed_session("s", "A", "test")
            .unwrap();
        state.attach_usage_database(db.clone()).await;
        let execution = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        let capture = state
            .pending_captures
            .lock()
            .await
            .values()
            .next()
            .unwrap()
            .capture
            .clone();
        let gate = capture.hold_after_snapshot().await;
        process
            .client()
            .request("test/complete", json!({"turnId":execution.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while state
                .pending_captures
                .lock()
                .await
                .values()
                .any(|capture| capture.writers_may_run)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        process.client().request("test/notify", json!({"method":"thread/tokenUsage/updated","params":{"threadId":"thread-1","tokenUsage":{"total":{"inputTokens":150,"outputTokens":0}}}})).await.unwrap();
        let persisted = tokio::time::timeout(Duration::from_millis(300), async {
            while db
                .lock()
                .await
                .get_session("s")
                .unwrap()
                .unwrap()
                .total_input_tokens
                != 150
            {
                tokio::task::yield_now().await;
            }
        })
        .await;
        drop(gate);
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            persisted.is_ok(),
            "after-snapshot wait blocked known-usage persistence"
        );
    }

    #[tokio::test]
    async fn runtime_regression_terminal_delayed_by_snapshot_keeps_old_generation() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let mut first = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        let capture = state
            .pending_captures
            .lock()
            .await
            .values()
            .next()
            .unwrap()
            .capture
            .clone();
        let gate = capture.hold_after_snapshot().await;
        state.interrupt_session_turn("s").await.unwrap();
        assert!(matches!(
            first.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        state.finish_session_turn("s", &first.generation).await;
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while state
                .pending_captures
                .lock()
                .await
                .values()
                .any(|capture| capture.writers_may_run)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut second = state.start_turn("s", options(&root)).await.unwrap();
        drop(gate);
        tokio::time::timeout(Duration::from_secs(2), async {
            while state
                .pending_captures
                .lock()
                .await
                .contains_key(&capture.key)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        process.client().request("test/notify", json!({"method":"item/agentMessage/delta","params":{"threadId":second.thread_id,"turnId":second.turn_id,"delta":"replacement"}})).await.unwrap();
        let mut replacement_live = false;
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(200), second.events.recv()).await
        {
            assert!(
                !is_terminal_event(&event),
                "delayed old terminal reached replacement"
            );
            if matches!(event, WorkBuddyEvent::TextDelta { delta } if delta == "replacement") {
                replacement_live = true;
                break;
            }
        }
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(replacement_live);
    }

    #[tokio::test]
    async fn runtime_regression_retirement_failure_keeps_unknown_writers_protected() {
        let (mut state, root) = fixture("turn/start", r#"{"result":{"turn":{"id":"held"}}}"#);
        let process = state.ensure_started().await.unwrap();
        state.rpc_timeout = Duration::from_millis(150);
        let running_state = state.clone();
        let opts = options(&root);
        let running = tokio::spawn(async move { running_state.start_turn("s", opts).await });
        wait_file(&root.join("waiting")).await;
        let gate = process.hold_shutdown().await;
        state.interrupt_session_turn("s").await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(state
            .pending_captures
            .lock()
            .await
            .values()
            .any(|capture| capture.writers_may_run));
        assert!(matches!(
            state.start_turn("s", options(&root)).await,
            Err(CodexRuntimeStateError::TurnAlreadyActive)
        ));
        let probe = RuntimeTurnCapture::begin(root.to_str().unwrap(), "still-protected").await;
        assert!(probe.before_error.is_some());
        drop(gate);
        assert!(process.is_running().await.unwrap());
        state.shutdown().await.unwrap();
        assert!(!process.is_running().await.unwrap());
        let _ = probe.close_after_stop().await;
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_late_interrupt_timeout_does_not_retire_replacement() {
        let (mut state, root) = fixture("turn/interrupt", r#"{"result":{}}"#);
        let mut first = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        state.rpc_timeout = Duration::from_millis(400);
        let stopping = state.clone();
        let stop = tokio::spawn(async move { stopping.interrupt_session_turn("s").await });
        wait_file(&root.join("waiting")).await;
        process.client().request("test/notify", json!({"method":"item/agentMessage/delta","params":{"threadId":first.thread_id,"turnId":first.turn_id,"delta":"old"}})).await.unwrap();
        assert!(matches!(
            first.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        state.finish_session_turn("s", &first.generation).await;
        process
            .client()
            .request("test/complete", json!({"turnId":first.turn_id}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.pending_captures.lock().await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let second = state.start_turn("s", options(&root)).await.unwrap();
        assert!(stop.await.unwrap().is_err());
        assert!(process.is_running().await.unwrap());
        assert!(state
            .subscribers
            .lock()
            .await
            .contains_key(&second.thread_id));
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_usage_after_subscriber_cleanup_is_durable_and_deduplicated() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let path = root.join("usage.db");
        let db = Arc::new(Mutex::new(crate::store::Database::open(&path).unwrap()));
        db.lock()
            .await
            .ensure_fixed_session("s", "A", "test")
            .unwrap();
        state.attach_usage_database(db.clone()).await;
        let mut execution = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        state.interrupt_session_turn("s").await.unwrap();
        assert!(matches!(
            execution.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        state.finish_session_turn("s", &execution.generation).await;
        assert!(!state.subscribers.lock().await.contains_key("thread-1"));
        for input in [150, 150, 40, 200] {
            process.client().request("test/notify", json!({"method":"thread/tokenUsage/updated","params":{"threadId":"thread-1","tokenUsage":{"total":{"inputTokens":input,"outputTokens":5}}}})).await.unwrap();
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while db
                .lock()
                .await
                .get_session("s")
                .unwrap()
                .unwrap()
                .total_input_tokens
                != 200
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        state.shutdown().await.unwrap();
        let reopened = crate::store::Database::open(&path).unwrap();
        reopened
            .record_codex_usage("s", "thread-1", 200, 5)
            .unwrap();
        let total = reopened
            .usage_overview("s", root.to_str().unwrap())
            .unwrap();
        assert_eq!(total.session_input_tokens, 200);
        assert_eq!(total.session_output_tokens, 5);
        drop(reopened);
        drop(state);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_cancelled_known_usage_blocks_project_budget() {
        let (state, root) = fixture("turn/interrupt", r#"{"result":{}}"#);
        let db = Arc::new(Mutex::new(
            crate::store::Database::open_in_memory().unwrap(),
        ));
        state.attach_usage_database(db.clone()).await;
        {
            let db = db.lock().await;
            db.ensure_fixed_session("s", "A", "test").unwrap();
            db.ensure_fixed_session("b", "B", "test").unwrap();
            let mut budget = db
                .usage_overview("s", root.to_str().unwrap())
                .unwrap()
                .budget;
            budget.project_tokens = 100;
            db.set_usage_budget("s", root.to_str().unwrap(), budget)
                .unwrap();
        }
        let mut execution = state.start_turn("s", options(&root)).await.unwrap();
        let process = state.ensure_started().await.unwrap();
        let stopping = state.clone();
        let stop = tokio::spawn(async move { stopping.interrupt_session_turn("s").await });
        wait_file(&root.join("waiting")).await;
        process.client().request("test/notify", json!({"method":"thread/tokenUsage/updated","params":{"threadId":"thread-1","tokenUsage":{"total":{"inputTokens":150,"outputTokens":0}}}})).await.unwrap();
        let event = execution.events.recv().await.unwrap();
        if let WorkBuddyEvent::ContextUsage {
            cumulative_input_tokens,
            cumulative_output_tokens,
            ..
        } = event
        {
            db.lock()
                .await
                .record_codex_usage(
                    "s",
                    "thread-1",
                    cumulative_input_tokens,
                    cumulative_output_tokens,
                )
                .unwrap();
        }
        let known = db
            .lock()
            .await
            .usage_overview("b", root.to_str().unwrap())
            .unwrap();
        fs::write(root.join("release"), "").unwrap();
        stop.await.unwrap().unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            known.project_tokens, 150,
            "already reported usage was lost on cancellation"
        );
        assert!(
            known.check().is_err(),
            "another project session escaped the budget"
        );
    }

    async fn headless_known_usage_on_error(phase: &str) {
        let (state, root) = fixture(phase, r#"{"result":{}}"#);
        let db = Arc::new(Mutex::new(
            crate::store::Database::open_in_memory().unwrap(),
        ));
        state.attach_usage_database(db.clone()).await;
        db.lock()
            .await
            .ensure_fixed_session("s", "headless", "test")
            .unwrap();
        let opts = headless_options(&root);
        let (sender, mut observed) = mpsc::channel(16);
        let running_state = state.clone();
        let running = tokio::spawn(async move {
            running_state
                .execute_headless_observed("s", opts, Some(sender))
                .await
        });
        if phase == "exec/cancel" {
            while let Some(event) = observed.recv().await {
                if event["type"] == "turn.completed" {
                    break;
                }
            }
            assert!(state.interrupt_headless("s").await);
        }
        let result = running.await.unwrap();
        let total = db
            .lock()
            .await
            .usage_overview("s", root.to_str().unwrap())
            .unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
        assert_eq!(
            total.project_tokens, 7,
            "known headless usage disappeared on {phase}"
        );
    }

    #[tokio::test]
    async fn runtime_regression_headless_failed_known_usage_is_persisted() {
        headless_known_usage_on_error("exec/failure").await;
    }
    #[tokio::test]
    async fn runtime_regression_headless_cancelled_known_usage_is_persisted() {
        headless_known_usage_on_error("exec/cancel").await;
    }
    #[tokio::test]
    async fn runtime_regression_headless_read_error_keeps_parsed_usage() {
        headless_known_usage_on_error("exec/read-error").await;
    }

    async fn stopped_during(phase: &str, resume: bool) {
        let response = if phase == "initialize" {
            r#"{"result":{}}"#
        } else if phase == "turn/start" {
            r#"{"result":{"turn":{"id":"late-turn"}}}"#
        } else {
            r#"{"result":{"thread":{"id":"thread-1"}}}"#
        };
        let (state, root) = fixture(phase, response);
        if resume {
            state
                .sessions
                .bind(NewCodexSessionBinding {
                    workbuddy_session_id: "s",
                    codex_thread_id: "thread-1",
                    workspace_root: "/work",
                    provider: "openai",
                    model: "test",
                })
                .unwrap();
        }
        let running_state = state.clone();
        let opts = options(&root);
        let running = tokio::spawn(async move { running_state.start_turn("s", opts).await });
        wait_file(&root.join("waiting")).await;
        let registered = state.active_turns.lock().await.contains_key("s");
        let startup_process = if phase == "turn/start" {
            state.process.lock().await.clone()
        } else {
            None
        };
        state.interrupt_session_turn("s").await.unwrap();
        fs::write(root.join("release"), "").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .unwrap()
            .unwrap();
        let requests = fs::read_to_string(root.join("requests")).unwrap();
        let retired_before_cleanup = match startup_process {
            Some(process) => !process.is_running().await.unwrap(),
            None => false,
        };
        let cleaned = state.active_turns.lock().await.is_empty()
            && state.subscribers.lock().await.is_empty()
            && state.approval_preferences.lock().await.is_empty();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(registered, "cancellation must be registered before {phase}");
        assert!(
            result.is_err(),
            "stopped startup must not return a live execution"
        );
        assert!(
            cleaned,
            "failed or stopped startup must clean all per-turn state"
        );
        if phase == "turn/start" {
            assert!(
                requests.contains("turn/interrupt") || retired_before_cleanup,
                "uncertain turn/start must interrupt the returned ID or reap the process before cleanup"
            );
        } else {
            assert!(
                !requests.contains("turn/start"),
                "stop must prevent turn/start"
            );
        }
    }

    #[tokio::test]
    async fn runtime_regression_stop_during_initialization() {
        stopped_during("initialize", false).await;
    }
    #[tokio::test]
    async fn runtime_regression_stop_during_thread_start() {
        stopped_during("thread/start", false).await;
    }
    #[tokio::test]
    async fn runtime_regression_stop_during_resume() {
        stopped_during("thread/resume", true).await;
    }
    #[tokio::test]
    async fn runtime_regression_stop_during_late_turn_response() {
        stopped_during("turn/start", false).await;
    }

    #[tokio::test]
    async fn runtime_regression_missing_turn_id_cleans_preferences_and_subscriber() {
        let (state, root) = fixture("turn/start", r#"{"result":{}}"#);
        fs::write(root.join("release"), "").unwrap();
        let result = state.start_turn("s", options(&root)).await;
        let clean = state.active_turns.lock().await.is_empty()
            && state.subscribers.lock().await.is_empty()
            && state.approval_preferences.lock().await.is_empty();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
        assert!(clean, "invalid start response leaked turn state");
    }

    #[tokio::test]
    async fn runtime_regression_active_stop_publishes_cancelled_without_remote_notification() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let mut execution = state.start_turn("s", options(&root)).await.unwrap();
        state.interrupt_session_turn("s").await.unwrap();
        let event = tokio::time::timeout(Duration::from_millis(200), execution.events.recv()).await;
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(
            matches!(event, Ok(Some(WorkBuddyEvent::Cancelled))),
            "acknowledged stop must not depend on a later remote event"
        );
    }

    #[tokio::test]
    async fn runtime_regression_start_failure_releases_reservation_for_retry() {
        let (state, root) = fixture(
            "thread/start",
            r#"{"error":{"code":-1,"message":"test failure"}}"#,
        );
        fs::write(root.join("release"), "").unwrap();
        for _ in 0..2 {
            assert!(state.start_turn("s", options(&root)).await.is_err());
            assert!(state.active_turns.lock().await.is_empty());
            assert!(state.subscribers.lock().await.is_empty());
            assert!(state.approval_preferences.lock().await.is_empty());
            assert!(state.pending_captures.lock().await.is_empty());
        }
        let journal =
            crate::file_journal::FileJournal::open(&root, root.join(".xiaoheng/journal.db"))
                .unwrap();
        assert_eq!(
            journal.review("s", None).unwrap().status,
            "complete",
            "failed startup must still close its baseline"
        );
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_duplicate_send_during_startup_is_rejected() {
        let (state, root) = fixture("thread/start", r#"{"result":{"thread":{"id":"thread-1"}}}"#);
        let other = state.clone();
        let opts = options(&root);
        let first = tokio::spawn(async move { other.start_turn("s", opts).await });
        wait_file(&root.join("waiting")).await;
        let second = state.start_turn("s", options(&root)).await;
        state.interrupt_session_turn("s").await.unwrap();
        fs::write(root.join("release"), "").unwrap();
        assert!(first.await.unwrap().is_err());
        assert!(matches!(
            second,
            Err(CodexRuntimeStateError::TurnAlreadyActive)
        ));
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn headless_options(root: &Path) -> CodexExecOptions {
        CodexExecOptions {
            workspace_root: root.into(),
            provider: "custom".into(),
            model: "test".into(),
            prompt: "test".into(),
            permission_mode: CodexPermissionMode::FullAccess,
            allow_outside_workspace: false,
            attachments: vec![],
            thread_id: None,
            extra_environment: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn runtime_regression_cross_mode_reservations_do_not_overlap_one_session() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        state
            .active_execs
            .lock()
            .await
            .insert("s".into(), Arc::new(AtomicBool::new(false)));
        let foreground = state.register_turn("s").await;
        if let Ok(generation) = &foreground {
            state.finish_session_turn("s", generation).await;
        }
        state.active_execs.lock().await.clear();
        let generation = state.register_turn("s").await.unwrap();
        let headless = state.execute_headless("s", headless_options(&root)).await;
        state.finish_session_turn("s", &generation).await;
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert!(matches!(
            foreground,
            Err(CodexRuntimeStateError::TurnAlreadyActive)
        ));
        assert!(matches!(
            headless,
            Err(CodexRuntimeStateError::TurnAlreadyActive)
        ));
    }

    #[tokio::test]
    async fn runtime_regression_headless_stop_before_process_spawn_is_retained() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let environment = state.environment.lock().await;
        let running_state = state.clone();
        let opts = headless_options(&root);
        let running = tokio::spawn(async move { running_state.execute_headless("s", opts).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !state.active_execs.lock().await.contains_key("s") {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(state.interrupt_headless("s").await);
        drop(environment);
        let result = tokio::time::timeout(Duration::from_secs(2), running)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(state.active_execs.lock().await.is_empty());
        assert!(
            !root.join("requests").exists(),
            "stopped exec must not spawn a child"
        );
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_regression_headless_launch_failure_cleans_reservation() {
        let (mut state, root) = fixture("unused", r#"{"result":{}}"#);
        state.executable_override = Some(root.join("missing-codex"));
        assert!(state
            .execute_headless("s", headless_options(&root))
            .await
            .is_err());
        assert!(state.active_execs.lock().await.is_empty());
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_capture_main_waits_for_the_real_remote_terminal() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let mut execution = state.start_turn("s", options(&root)).await.unwrap();
        let journal =
            crate::file_journal::FileJournal::open(&root, root.join(".xiaoheng/journal.db"))
                .unwrap();
        let started_status = journal.review("s", None).unwrap().status;
        state.interrupt_session_turn("s").await.unwrap();
        assert!(matches!(
            execution.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        let local_stop_status = journal.review("s", None).unwrap().status;
        state
            .ensure_started()
            .await
            .unwrap()
            .client()
            .request("test/complete", json!({}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while journal.review("s", None).unwrap().status != "complete" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .ok();
        let completed = journal.review("s", None).unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            started_status, "capturing",
            "runtime start must await baseline capture"
        );
        assert_ne!(
            local_stop_status, "complete",
            "local cancellation must not fabricate an after snapshot"
        );
        assert_eq!(completed.status, "complete");
        assert!(completed
            .changes
            .iter()
            .any(|change| change.path == "generated.txt"));
    }

    #[tokio::test]
    async fn runtime_capture_local_cancel_is_unavailable_then_releases_at_remote_stop() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let mut execution = state.start_turn("s", options(&root)).await.unwrap();
        state.interrupt_session_turn("s").await.unwrap();
        assert!(matches!(
            execution.events.recv().await,
            Some(WorkBuddyEvent::Cancelled)
        ));
        state.finish_session_turn("s", &execution.generation).await;
        let journal =
            crate::file_journal::FileJournal::open(&root, root.join(".xiaoheng/journal.db"))
                .unwrap();
        assert_eq!(journal.review("s", None).unwrap().status, "unavailable");
        assert!(
            !state.pending_captures.lock().await.is_empty(),
            "unknown writers must retain their guard"
        );
        state
            .ensure_started()
            .await
            .unwrap()
            .client()
            .request("test/complete", json!({}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !state.pending_captures.lock().await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let next = RuntimeTurnCapture::begin(root.to_str().unwrap(), "next").await;
        assert!(
            next.before_error.is_none(),
            "actual terminal must release unavailable capture guards"
        );
        next.finish().await.unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_capture_headless_before_after_and_busy_downgrade() {
        let (state, root) = fixture("unused", r#"{"result":{}}"#);
        let result = state
            .execute_headless("s", headless_options(&root))
            .await
            .unwrap();
        assert_eq!(result.thread_id.as_deref(), Some("real-headless-thread"));
        let journal =
            crate::file_journal::FileJournal::open(&root, root.join(".xiaoheng/journal.db"))
                .unwrap();
        let review = journal.review("s", None).unwrap();
        assert_eq!(review.status, "complete");
        assert!(review
            .changes
            .iter()
            .any(|change| change.path == "headless-generated.txt"));
        let blocker = RuntimeTurnCapture::begin(root.to_str().unwrap(), "other").await;
        assert!(
            state
                .execute_headless("s", headless_options(&root))
                .await
                .is_ok(),
            "busy capture must not block an ordinary task"
        );
        assert_eq!(journal.review("s", None).unwrap().status, "unavailable");
        let _ = blocker.finish().await;
        let next = RuntimeTurnCapture::begin(root.to_str().unwrap(), "next").await;
        assert!(
            next.before_error.is_none(),
            "all completed busy tuples must release their guards"
        );
        next.finish().await.unwrap();
        state.shutdown().await.unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
