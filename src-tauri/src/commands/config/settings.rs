use crate::store::settings::SshServerConfig;
use crate::store::{AppState, Settings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderModelInfo {
    pub id: String,
    pub label: String,
    pub owned_by: Option<String>,
    pub kind: String,
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let settings = state.settings.lock().await;
    Ok(settings.clone())
}

#[tauri::command]
pub async fn get_default_workspace() -> Result<String, String> {
    Ok(harmony_default_workspace())
}

fn harmony_default_workspace() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("harmony")
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: Value,
) -> Result<Settings, String> {
    let saved = save_settings_with(
        &state.settings,
        &updates,
        validate_vision_settings,
        |settings| {
            settings.save().map_err(|error| error.to_string())?;
            crate::agent_core::approval::sync_confirm_flags(
                &state.confirm_flags,
                settings.confirm_shell_commands,
                settings.confirm_file_writes,
            );
            Ok(())
        },
    )
    .await?;
    if let Err(error) =
        crate::commands::platform::system::ensure_workspace_shortcut(&saved.workspace_root)
    {
        let warning = format!("设置已保存，但创建工作区桌面快捷方式失败：{error}");
        tracing::warn!("{}", warning);
        let _ = app.emit("settings_save_warning", warning);
    } else {
        let _ = app.emit("workspace_shortcut_ready", &saved.workspace_root);
    }
    Ok(saved)
}

async fn save_settings_with<V, F, P>(
    shared: &tokio::sync::Mutex<Settings>,
    updates: &Value,
    validate: V,
    persist: P,
) -> Result<Settings, String>
where
    V: FnOnce(Settings) -> F,
    F: std::future::Future<Output = Result<(), String>>,
    P: FnOnce(&Settings) -> Result<(), String>,
{
    let original = shared.lock().await.clone();
    let mut candidate = original.clone();
    apply_settings_patch(&mut candidate, updates);
    candidate.migrate_public_service_urls();
    if vision_target(&candidate).is_some() && vision_target(&original) != vision_target(&candidate)
    {
        validate(candidate.clone()).await?;
    }
    let mut current = shared.lock().await;
    if serde_json::to_value(&*current).map_err(|e| e.to_string())?
        != serde_json::to_value(&original).map_err(|e| e.to_string())?
        || current.config_path != original.config_path
    {
        return Err("设置在校验期间已变更，请重试保存。".into());
    }
    persist(&candidate)?;
    *current = candidate.clone();
    Ok(candidate)
}

fn vision_target(settings: &Settings) -> Option<(bool, String, String, String, String)> {
    if !settings.vision_enabled {
        return None;
    }
    Some(if settings.vision_use_main_llm {
        (
            true,
            settings.provider.clone(),
            settings.model.clone(),
            settings.active_api_key().into(),
            settings.custom_base_url.clone(),
        )
    } else {
        (
            false,
            settings.vision_provider.clone(),
            settings.vision_model.clone(),
            settings.vision_api_key.clone(),
            settings.vision_base_url.clone(),
        )
    })
}

async fn validate_vision_settings(settings: Settings) -> Result<(), String> {
    if settings.vision_use_main_llm {
        // User chose to use main LLM as vision model — validate if vision is enabled
        if settings.vision_enabled {
            let main_api_key = settings.active_api_key().to_string();
            if !main_api_key.is_empty() && !settings.model.is_empty() {
                let main_base_url = if settings.custom_base_url.is_empty() {
                    None
                } else {
                    Some(settings.custom_base_url.as_str())
                };
                match crate::commands::chat::validate_vision_model(
                    &settings.provider,
                    &main_api_key,
                    &settings.model,
                    main_base_url,
                )
                .await
                {
                    Ok(()) => {
                        info!(
                            "Vision validation: main model '{}' supports vision",
                            settings.model
                        );
                    }
                    Err(msg) => {
                        tracing::warn!("Vision validation failed: {}", msg);
                        return Err(format!(
                "主模型 '{}' 不支持视觉功能，无法启用。请配置独立的视觉模型，或更换支持视觉的主模型。\nTechnical: {}",
                settings.model, msg
                ));
                    }
                }
            }
        }
    } else {
        // User configured a separate vision model — validate it
        if !settings.vision_provider.is_empty()
            && !settings.vision_model.is_empty()
            && !settings.vision_api_key.is_empty()
        {
            let vis_base_url = if settings.vision_base_url.is_empty() {
                None
            } else {
                Some(settings.vision_base_url.as_str())
            };
            match crate::commands::chat::validate_vision_model(
                &settings.vision_provider,
                &settings.vision_api_key,
                &settings.vision_model,
                vis_base_url,
            )
            .await
            {
                Ok(()) => {
                    info!(
                        "Vision validation: separate model '{}' supports vision",
                        settings.vision_model
                    );
                }
                Err(msg) => {
                    tracing::warn!("Vision validation failed: {}", msg);
                    return Err(format!(
                "独立视觉模型 '{}' 不支持视觉功能，无法保存配置。请检查模型名称是否正确，或更换支持视觉的模型。\nTechnical: {}",
                settings.vision_model, msg
            ));
                }
            }
        }
    }
    Ok(())
}
fn apply_settings_patch(settings: &mut Settings, updates: &Value) {
    // LLM provider keys — only overwrite if the incoming value is non-empty.
    // An empty string means "unchanged" (the frontend may not send the decrypted key back).
    if let Some(v) = updates["anthropic_api_key"].as_str() {
        if !v.is_empty() {
            settings.anthropic_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["openai_api_key"].as_str() {
        if !v.is_empty() {
            settings.openai_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["deepseek_api_key"].as_str() {
        if !v.is_empty() {
            settings.deepseek_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["qwen_api_key"].as_str() {
        if !v.is_empty() {
            settings.qwen_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["minimax_api_key"].as_str() {
        if !v.is_empty() {
            settings.minimax_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["zhipu_api_key"].as_str() {
        if !v.is_empty() {
            settings.zhipu_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["kimi_api_key"].as_str() {
        if !v.is_empty() {
            settings.kimi_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["siliconflow_api_key"].as_str() {
        if !v.is_empty() {
            settings.siliconflow_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["harmony_api_key"].as_str() {
        if !v.is_empty() {
            settings.harmony_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["memory_embedding_model"].as_str() {
        settings.memory_embedding_model = v.trim().to_string();
    }
    if let Some(v) = updates["harvis_image_model"].as_str() {
        settings.harvis_image_model = v.to_string();
    }
    if let Some(v) = updates["harvis_video_model"].as_str() {
        settings.harvis_video_model = v.to_string();
    }
    if let Some(v) = updates["harvis_music_model"].as_str() {
        settings.harvis_music_model = v.to_string();
    }
    if let Some(v) = updates["harvis_tts_model"].as_str() {
        settings.harvis_tts_model = v.to_string();
    }
    if let Some(v) = updates["provider"].as_str() {
        settings.provider = v.to_string();
    }
    if let Some(v) = updates["model"].as_str() {
        settings.model = v.to_string();
    }
    if let Some(v) = updates["enabled_models"].as_array() {
        settings.enabled_models = string_array(v);
    }
    if let Some(v) = updates["available_models"].as_array() {
        settings.available_models = string_array(v);
    }
    if let Some(v) = updates["custom_base_url"].as_str() {
        settings.custom_base_url = v.to_string();
    }
    if let Some(v) = updates["workspace_root"].as_str() {
        // workspace_root must never be empty — fall back to default if blank
        let resolved = if v.trim().is_empty() {
            harmony_default_workspace()
        } else {
            v.to_string()
        };
        settings.workspace_root = resolved;
    }
    if let Some(v) = updates["allow_outside_workspace"].as_bool() {
        settings.allow_outside_workspace = v;
    }
    if let Some(v) = updates["language"].as_str() {
        settings.language = v.to_string();
    }
    if let Some(v) = updates["max_tokens"].as_u64() {
        settings.max_tokens = v as u32;
    }
    if let Some(v) = updates["context_window"].as_u64() {
        settings.context_window = v as u32;
    }
    if let Some(v) = updates["confirm_shell_commands"].as_bool() {
        settings.confirm_shell_commands = v;
    }
    if let Some(v) = updates["confirm_file_writes"].as_bool() {
        settings.confirm_file_writes = v;
    }
    if let Some(v) = updates["confirm_mcp_tool_calls"].as_bool() {
        settings.confirm_mcp_tool_calls = v;
    }
    if let Some(v) = updates["browser_headless"].as_bool() {
        settings.browser_headless = v;
    }
    if let Some(v) = updates["im_auto_minimal_mode"].as_bool() {
        settings.im_auto_minimal_mode = v;
    }
    if let Some(v) = updates["im_message_mode"].as_str() {
        settings.im_message_mode = v.to_string();
    }
    if let Some(v) = updates["policy_mode"].as_str() {
        settings.policy_mode = v.to_string();
    }
    if let Some(v) = updates["tool_rate_limit_per_minute"].as_u64() {
        settings.tool_rate_limit_per_minute = v as u32;
    }
    if let Some(map) = updates["builtin_tool_enabled"].as_object() {
        settings.builtin_tool_enabled = map
            .iter()
            .filter_map(|(k, v)| v.as_bool().map(|b| (k.clone(), b)))
            .collect();
    }
    // Feishu
    if let Some(v) = updates["feishu_app_id"].as_str() {
        settings.feishu_app_id = v.to_string();
    }
    if let Some(v) = updates["feishu_app_secret"].as_str() {
        if !v.is_empty() {
            settings.feishu_app_secret = v.to_string();
        }
    }
    if let Some(v) = updates["feishu_domain"].as_str() {
        settings.feishu_domain = v.to_string();
    }
    if let Some(v) = updates["feishu_enabled"].as_bool() {
        settings.feishu_enabled = v;
    }
    // WeCom
    if let Some(v) = updates["wecom_bot_id"].as_str() {
        settings.wecom_bot_id = v.to_string();
    }
    if let Some(v) = updates["wecom_bot_secret"].as_str() {
        if !v.is_empty() {
            settings.wecom_bot_secret = v.to_string();
        }
    }
    if let Some(v) = updates["wecom_enabled"].as_bool() {
        settings.wecom_enabled = v;
    }
    // DingTalk
    if let Some(v) = updates["dingtalk_app_key"].as_str() {
        settings.dingtalk_app_key = v.to_string();
    }
    if let Some(v) = updates["dingtalk_app_secret"].as_str() {
        if !v.is_empty() {
            settings.dingtalk_app_secret = v.to_string();
        }
    }
    if let Some(v) = updates["dingtalk_robot_code"].as_str() {
        if !v.is_empty() {
            settings.dingtalk_robot_code = v.to_string();
        }
    }
    if let Some(v) = updates["dingtalk_corp_id"].as_str() {
        settings.dingtalk_corp_id = v.to_string();
    }
    if let Some(v) = updates["dingtalk_agent_id"].as_str() {
        settings.dingtalk_agent_id = v.to_string();
    }
    if let Some(v) = updates["dingtalk_mcp_url"].as_str() {
        settings.dingtalk_mcp_url = v.to_string();
    }
    if let Some(v) = updates["dingtalk_enabled"].as_bool() {
        settings.dingtalk_enabled = v;
    }
    // Telegram
    if let Some(v) = updates["telegram_bot_token"].as_str() {
        if !v.is_empty() {
            settings.telegram_bot_token = v.to_string();
        }
    }
    if let Some(v) = updates["telegram_enabled"].as_bool() {
        settings.telegram_enabled = v;
    }
    // Slack
    if let Some(v) = updates["slack_webhook_url"].as_str() {
        settings.slack_webhook_url = v.to_string();
    }
    if let Some(v) = updates["slack_enabled"].as_bool() {
        settings.slack_enabled = v;
    }
    // Discord
    if let Some(v) = updates["discord_webhook_url"].as_str() {
        settings.discord_webhook_url = v.to_string();
    }
    if let Some(v) = updates["discord_enabled"].as_bool() {
        settings.discord_enabled = v;
    }
    // Teams
    if let Some(v) = updates["teams_webhook_url"].as_str() {
        settings.teams_webhook_url = v.to_string();
    }
    if let Some(v) = updates["teams_enabled"].as_bool() {
        settings.teams_enabled = v;
    }
    // Matrix
    if let Some(v) = updates["matrix_homeserver"].as_str() {
        settings.matrix_homeserver = v.to_string();
    }
    if let Some(v) = updates["matrix_access_token"].as_str() {
        if !v.is_empty() {
            settings.matrix_access_token = v.to_string();
        }
    }
    if let Some(v) = updates["matrix_room_id"].as_str() {
        settings.matrix_room_id = v.to_string();
    }
    if let Some(v) = updates["matrix_enabled"].as_bool() {
        settings.matrix_enabled = v;
    }
    // Generic webhook
    if let Some(v) = updates["webhook_outbound_url"].as_str() {
        settings.webhook_outbound_url = v.to_string();
    }
    if let Some(v) = updates["webhook_auth_token"].as_str() {
        if !v.is_empty() {
            settings.webhook_auth_token = v.to_string();
        }
    }
    if let Some(v) = updates["webhook_enabled"].as_bool() {
        settings.webhook_enabled = v;
    }
    // WeChat (iLink Bot)
    if let Some(v) = updates["wechat_enabled"].as_bool() {
        settings.wechat_enabled = v;
    }
    if let Some(v) = updates["wechat_gateway_token"].as_str() {
        if !v.is_empty() {
            settings.wechat_gateway_token = v.to_string();
        }
    }
    if let Some(v) = updates["wechat_gateway_port"].as_u64() {
        settings.wechat_gateway_port = v as u16;
    }
    if let Some(v) = updates["wechat_bot_token"].as_str() {
        if !v.is_empty() {
            settings.wechat_bot_token = v.to_string();
        }
    }
    if let Some(v) = updates["wechat_base_url"].as_str() {
        settings.wechat_base_url = v.to_string();
    }
    if let Some(v) = updates["wechat_bot_id"].as_str() {
        settings.wechat_bot_id = v.to_string();
    }
    // Email (SMTP / IMAP)
    if let Some(v) = updates["smtp_host"].as_str() {
        settings.smtp_host = v.to_string();
    }
    if let Some(v) = updates["smtp_port"].as_u64() {
        settings.smtp_port = v as u16;
    }
    if let Some(v) = updates["smtp_username"].as_str() {
        settings.smtp_username = v.to_string();
    }
    if let Some(v) = updates["smtp_password"].as_str() {
        if !v.is_empty() {
            settings.smtp_password = v.to_string();
        }
    }
    if let Some(v) = updates["imap_host"].as_str() {
        settings.imap_host = v.to_string();
    }
    if let Some(v) = updates["imap_port"].as_u64() {
        settings.imap_port = v as u16;
    }
    if let Some(v) = updates["smtp_from_name"].as_str() {
        settings.smtp_from_name = v.to_string();
    }
    if let Some(v) = updates["email_enabled"].as_bool() {
        settings.email_enabled = v;
    }

    if let Some(v) = updates["allow_multiple_instances"].as_bool() {
        settings.allow_multiple_instances = v;
    }
    if let Some(v) = updates["fallback_models"].as_array() {
        settings.fallback_models = v
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .filter(|s| !s.trim().is_empty())
            .collect();
    }

    // Agent loop
    if let Some(v) = updates["max_iterations"].as_u64() {
        settings.max_iterations = v as u32;
    }
    if let Some(v) = updates["auto_compact_input_tokens_threshold"].as_u64() {
        settings.auto_compact_input_tokens_threshold = v as u32;
    }
    if let Some(v) = updates["compaction_micro_percent"].as_u64() {
        settings.compaction_micro_percent = v.min(100) as u8;
    }
    if let Some(v) = updates["compaction_auto_percent"].as_u64() {
        settings.compaction_auto_percent = v.min(100) as u8;
    }
    if let Some(v) = updates["compaction_full_percent"].as_u64() {
        settings.compaction_full_percent = v.min(100) as u8;
    }
    if let Some(v) = updates["max_tool_result_tokens"].as_u64() {
        settings.max_tool_result_tokens = v as u32;
    }
    if let Some(v) = updates["summary_model"].as_str() {
        settings.summary_model = if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        };
    } else if updates
        .get("summary_model")
        .map(|v| v.is_null())
        .unwrap_or(false)
    {
        // Explicit null → clear the override
        settings.summary_model = None;
    }
    if let Some(v) = updates["project_instruction_budget_chars"].as_u64() {
        settings.project_instruction_budget_chars = v.max(512) as u32;
    }
    if let Some(v) = updates["enable_project_instructions"].as_bool() {
        settings.enable_project_instructions = v;
    }
    if let Some(v) = updates["xiaoheng_personal_prompt"]
        .as_str()
        .or_else(|| updates["piscis_personal_prompt"].as_str())
    {
        settings.xiaoheng_personal_prompt = v.to_string();
    }
    if let Some(v) = updates["llm_read_timeout_secs"].as_u64() {
        settings.llm_read_timeout_secs = v.max(30) as u32; // minimum 30s
    }
    if let Some(v) = updates["koi_timeout_secs"].as_u64() {
        settings.koi_timeout_secs = v.max(60) as u32; // minimum 60s
    }

    // Skill evolution
    if let Some(evo) = updates.get("skill_evolution").and_then(|v| v.as_object()) {
        if let Some(v) = evo.get("review_enabled").and_then(|x| x.as_bool()) {
            settings.skill_evolution.review_enabled = v;
        }
        if let Some(v) = evo.get("review_every_turn").and_then(|x| x.as_bool()) {
            settings.skill_evolution.review_every_turn = v;
        }
        if let Some(v) = evo
            .get("create_skill_min_tool_calls")
            .and_then(|x| x.as_u64())
        {
            settings.skill_evolution.create_skill_min_tool_calls = v.max(1) as u32;
        }
        if let Some(v) = evo
            .get("umbrella_skill_interval_turns")
            .and_then(|x| x.as_u64())
        {
            settings.skill_evolution.umbrella_skill_interval_turns = v.max(1) as u32;
        }
        if let Some(v) = evo.get("curator_interval_hours").and_then(|x| x.as_u64()) {
            settings.skill_evolution.curator_interval_hours = v.max(1) as u32;
        }
        if let Some(v) = evo.get("curator_min_idle_hours").and_then(|x| x.as_u64()) {
            settings.skill_evolution.curator_min_idle_hours = v as u32;
        }
        if let Some(v) = evo.get("stale_after_days").and_then(|x| x.as_u64()) {
            settings.skill_evolution.stale_after_days = v.max(1) as u32;
        }
        if let Some(v) = evo.get("archive_after_days").and_then(|x| x.as_u64()) {
            settings.skill_evolution.archive_after_days = v.max(1) as u32;
        }
        if let Some(v) = evo
            .get("curator_llm_merge_enabled")
            .and_then(|x| x.as_bool())
        {
            settings.skill_evolution.curator_llm_merge_enabled = v;
        }
    }

    // Heartbeat
    if let Some(v) = updates["heartbeat_enabled"].as_bool() {
        settings.heartbeat_enabled = v;
    }
    if let Some(v) = updates["heartbeat_interval_mins"].as_u64() {
        settings.heartbeat_interval_mins = v as u32;
    }
    if let Some(v) = updates["heartbeat_prompt"].as_str() {
        settings.heartbeat_prompt = v.to_string();
    }

    // Vision / multimodal
    if let Some(v) = updates["vision_enabled"].as_bool() {
        settings.vision_enabled = v;
    }
    if let Some(v) = updates["vision_use_main_llm"].as_bool() {
        settings.vision_use_main_llm = v;
    }
    if let Some(v) = updates["vision_provider"].as_str() {
        settings.vision_provider = v.to_string();
    }
    if let Some(v) = updates["vision_model"].as_str() {
        settings.vision_model = v.to_string();
    }
    if let Some(v) = updates["vision_api_key"].as_str() {
        if !v.is_empty() {
            settings.vision_api_key = v.to_string();
        }
    }
    if let Some(v) = updates["vision_base_url"].as_str() {
        settings.vision_base_url = v.to_string();
    }

    // Streaming output
    if let Some(v) = updates["enable_streaming"].as_bool() {
        settings.enable_streaming = v;
    }

    // SSH servers — full replacement when provided
    if let Some(arr) = updates["ssh_servers"].as_array() {
        let mut servers: Vec<SshServerConfig> = Vec::new();
        for item in arr {
            let id = item["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let existing_password = settings
                .ssh_servers
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.password.clone())
                .unwrap_or_default();
            let existing_key = settings
                .ssh_servers
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.private_key.clone())
                .unwrap_or_default();
            servers.push(SshServerConfig {
                id,
                label: item["label"].as_str().unwrap_or("").to_string(),
                host: item["host"].as_str().unwrap_or("").to_string(),
                port: item["port"].as_u64().unwrap_or(22) as u16,
                username: item["username"].as_str().unwrap_or("").to_string(),
                // Only update password/key if non-empty (frontend may omit to keep existing)
                password: {
                    let v = item["password"].as_str().unwrap_or("");
                    if item.get("password").is_some_and(Value::is_null) {
                        String::new()
                    } else if v.is_empty() {
                        existing_password
                    } else {
                        v.to_string()
                    }
                },
                private_key: {
                    let v = item["private_key"].as_str().unwrap_or("");
                    if item.get("private_key").is_some_and(Value::is_null) {
                        String::new()
                    } else if v.is_empty() {
                        existing_key
                    } else {
                        v.to_string()
                    }
                },
            });
        }
        settings.ssh_servers = servers;
    }

    // Named LLM providers — full replacement when provided, preserving existing api_key if blank
    if let Some(arr) = updates["llm_providers"].as_array() {
        let mut providers: Vec<crate::store::settings::LlmProviderConfig> = Vec::new();
        for item in arr {
            let id = item["id"].as_str().unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            // Keep the existing api_key if the frontend sends an empty string
            // (frontend never sends back decrypted keys after initial load)
            let existing_api_key = settings
                .llm_providers
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.api_key.clone())
                .unwrap_or_default();
            providers.push(crate::store::settings::LlmProviderConfig {
                id,
                label: item["label"].as_str().unwrap_or("").to_string(),
                provider: item["provider"].as_str().unwrap_or("openai").to_string(),
                model: item["model"].as_str().unwrap_or("").to_string(),
                enabled_models: item["enabled_models"]
                    .as_array()
                    .map(|arr| string_array(arr))
                    .unwrap_or_default(),
                available_models: item["available_models"]
                    .as_array()
                    .map(|arr| string_array(arr))
                    .unwrap_or_default(),
                api_key: {
                    let v = item["api_key"].as_str().unwrap_or("");
                    if item.get("api_key").is_some_and(Value::is_null) {
                        String::new()
                    } else if v.is_empty() {
                        existing_api_key
                    } else {
                        v.to_string()
                    }
                },
                base_url: item["base_url"].as_str().unwrap_or("").to_string(),
                max_tokens: item["max_tokens"].as_u64().unwrap_or(0) as u32,
            });
        }
        settings.llm_providers = providers;
    }

    // User tool configs
    if let Some(map) = updates["user_tool_configs"].as_object() {
        settings.user_tool_configs = map
            .iter()
            .filter_map(|(k, v)| {
                v.as_object()
                    .map(|obj| (k.clone(), serde_json::Value::Object(obj.clone())))
            })
            .collect();
    }

    // Missing/blank secrets preserve existing values. JSON null is an explicit clear.
    macro_rules! clear_secret {
        ($($field:ident),+ $(,)?) => { $(
            if updates.get(stringify!($field)).is_some_and(Value::is_null) {
                settings.$field.clear();
            }
        )+ };
    }
    clear_secret!(
        anthropic_api_key,
        openai_api_key,
        deepseek_api_key,
        qwen_api_key,
        minimax_api_key,
        zhipu_api_key,
        kimi_api_key,
        siliconflow_api_key,
        harmony_api_key,
        vision_api_key,
        feishu_app_secret,
        wecom_bot_secret,
        dingtalk_app_secret,
        dingtalk_robot_code,
        telegram_bot_token,
        matrix_access_token,
        webhook_auth_token,
        wechat_gateway_token,
        wechat_bot_token,
        smtp_password
    );
}

fn string_array(arr: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for item in arr {
        if let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            if !out.iter().any(|x| x == s) {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn provider_default_base_url(provider: &str) -> &'static str {
    match provider {
        "anthropic" => "https://api.anthropic.com/v1",
        "openai" => "https://api.openai.com/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        "qwen" | "tongyi" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "minimax" => "https://api.minimax.io/v1",
        "zhipu" => "https://api.z.ai/api/paas/v4",
        "kimi" | "moonshot" => "https://api.moonshot.cn/v1",
        "siliconflow" => "https://api.siliconflow.cn/v1",
        "harmony" => crate::service_urls::HARMONY_API_BASE,
        _ => "",
    }
}

fn normalize_model_base_url(provider: &str, base_url: Option<&str>) -> Result<String, String> {
    let raw = base_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| provider_default_base_url(provider));
    if raw.trim().is_empty() {
        return Err("Base URL is required for custom model providers".into());
    }
    Ok(crate::service_urls::migrate_public_service_url(raw)
        .trim_end_matches('/')
        .to_string())
}

/// 依据模型 ID 关键字识别模型种类，用于 Harvis 创作模型分类。
/// 返回 "chat" | "embedding" | "image" | "video" | "music" | "tts"。
pub fn classify_harvis_model_kind(id: &str) -> &'static str {
    let lower = id.to_ascii_lowercase();
    if lower.contains("embedding")
        || lower.contains("bge-")
        || lower.contains("gte-")
        || lower.contains("nomic-embed")
        || lower.contains("mxbai-embed")
        || lower.contains("e5-")
    {
        "embedding"
    } else if lower.contains("gpt-image")
        || lower.contains("dall-e")
        || lower.contains("flux")
        || lower.contains("stable-diffusion")
        || lower.contains("sd-")
    {
        "image"
    } else if lower.contains("seedance")
        || lower.contains("sora")
        || lower.contains("vidu")
        || lower.contains("kling")
        || lower.contains("hailuo")
        || lower.contains("runway")
    {
        "video"
    } else if lower.contains("suno") || lower.contains("udio") {
        "music"
    } else if lower.contains("tts") || lower.contains("speech") || lower.contains("cosyvoice") {
        "tts"
    } else {
        "chat"
    }
}

fn extract_models(value: &Value) -> Vec<ProviderModelInfo> {
    let arr = value
        .get("data")
        .and_then(|v| v.as_array())
        .or_else(|| value.get("models").and_then(|v| v.as_array()))
        .or_else(|| value.as_array());

    let mut out = Vec::new();
    if let Some(items) = arr {
        for item in items {
            let id = item
                .get("id")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("model"))
                .and_then(|v| v.as_str())
                .or_else(|| item.as_str())
                .map(str::trim)
                .unwrap_or("");
            if id.is_empty() || out.iter().any(|m: &ProviderModelInfo| m.id == id) {
                continue;
            }
            let label = item
                .get("display_name")
                .or_else(|| item.get("label"))
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(id)
                .to_string();
            let owned_by = item
                .get("owned_by")
                .or_else(|| item.get("owner"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let declared_kind = item
                .get("kind")
                .or_else(|| item.get("type"))
                .or_else(|| item.get("category"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let kind = if declared_kind.eq_ignore_ascii_case("embedding")
                || declared_kind.eq_ignore_ascii_case("embeddings")
            {
                "embedding"
            } else {
                classify_harvis_model_kind(id)
            };
            out.push(ProviderModelInfo {
                id: id.to_string(),
                label,
                owned_by,
                kind: kind.to_string(),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

#[tauri::command]
pub async fn fetch_provider_models(
    provider: String,
    api_key: String,
    base_url: Option<String>,
) -> Result<Vec<ProviderModelInfo>, String> {
    let provider = provider.trim().to_lowercase();
    let api_key = api_key.trim().to_string();
    if api_key.is_empty()
        && !(provider == "ollama"
            || (provider == "custom"
                && base_url
                    .as_deref()
                    .map(|u| u.contains("localhost") || u.contains("127.0.0.1"))
                    .unwrap_or(false)))
    {
        return Err("API Key is required before fetching models".into());
    }

    let base = normalize_model_base_url(&provider, base_url.as_deref())?;
    let url = format!("{}/models", base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let mut req = client.get(&url).header("Accept", "application/json");
    if provider == "anthropic" {
        req = req
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01");
    } else if !api_key.is_empty() {
        req = req.bearer_auth(&api_key);
    }

    let resp = req.send().await.map_err(|e| {
        format!(
            "failed to fetch model list (provider={}, url={}): {}",
            provider, url, e
        )
    })?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let preview: String = text.chars().take(800).collect();
        return Err(format!(
            "model list request failed (provider={}, status={}, url={}): {}",
            provider, status, url, preview
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        let preview: String = text.chars().take(800).collect();
        format!(
            "model list response is not valid JSON (provider={}, url={}): {}; body={}",
            provider, url, e, preview
        )
    })?;
    let models = extract_models(&value);
    if models.is_empty() {
        return Err(format!(
            "provider returned no models (provider={}, url={})",
            provider, url
        ));
    }
    Ok(models)
}

#[tauri::command]
pub async fn is_configured(state: State<'_, AppState>) -> Result<bool, String> {
    let settings = state.settings.lock().await;
    Ok(settings.is_configured())
}

#[cfg(test)]
mod model_catalog_tests {
    use super::{classify_harvis_model_kind, extract_models};
    use serde_json::json;

    #[test]
    fn embedding_models_are_classified_by_id_or_declared_type() {
        assert_eq!(
            classify_harvis_model_kind("Qwen/Qwen3-Embedding-8B"),
            "embedding"
        );
        assert_eq!(classify_harvis_model_kind("BAAI/bge-m3"), "embedding");
        let models = extract_models(&json!({"data": [
            {"id": "private-vector", "type": "embedding"},
            {"id": "deepseek-v4-flash", "type": "chat"}
        ]}));
        assert_eq!(models[1].kind, "embedding");
        assert_eq!(models[0].kind, "chat");
    }
}

#[cfg(test)]
mod account_settings_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn account_settings_concurrent_change_is_not_overwritten() {
        let shared = tokio::sync::Mutex::new(Settings::default());
        let result = save_settings_with(
            &shared,
            &json!({"vision_enabled":true, "language":"en"}),
            |_| async {
                shared.lock().await.wechat_bot_token = "newly-bound-token".into();
                Ok(())
            },
            |_| panic!("must not overwrite newer settings"),
        )
        .await;
        assert!(result.unwrap_err().contains("变更"));
        let current = shared.lock().await;
        assert_eq!(current.wechat_bot_token, "newly-bound-token");
        assert!(!current.vision_enabled);
    }

    #[tokio::test]
    async fn account_settings_effective_vision_changes_validate_but_unrelated_fields_do_not() {
        let mut original = Settings::default();
        original.vision_enabled = true;
        original.vision_use_main_llm = false;
        original.vision_provider = "openai".into();
        original.vision_model = "visual".into();
        original.vision_api_key = "key".into();
        original.vision_base_url = "https://old.example/v1".into();
        for patch in [
            json!({"language":"en"}),
            serde_json::to_value(&original).unwrap(),
        ] {
            let shared = tokio::sync::Mutex::new(original.clone());
            assert!(save_settings_with(
                &shared,
                &patch,
                |_| async { panic!("unchanged target must not validate") },
                |_| Ok(())
            )
            .await
            .is_ok());
        }
        for patch in [
            json!({"vision_base_url":"https://new.example/v1"}),
            json!({"vision_api_key":"new-key"}),
        ] {
            let shared = tokio::sync::Mutex::new(original.clone());
            let result = save_settings_with(
                &shared,
                &patch,
                |_| async { Err("validated".into()) },
                |_| panic!("validation failed"),
            )
            .await;
            assert_eq!(result.unwrap_err(), "validated");
        }
        original.vision_use_main_llm = true;
        for patch in [
            json!({"harmony_api_key":"main-key"}),
            json!({"custom_base_url":"https://main.example/v1"}),
        ] {
            let shared = tokio::sync::Mutex::new(original.clone());
            let result = save_settings_with(
                &shared,
                &patch,
                |_| async { Err("validated".into()) },
                |_| panic!("validation failed"),
            )
            .await;
            assert_eq!(result.unwrap_err(), "validated");
        }
        let shared = tokio::sync::Mutex::new(original);
        assert!(save_settings_with(
            &shared,
            &json!({"vision_enabled":false}),
            |_| async { panic!("disabling vision must not validate") },
            |_| Ok(())
        )
        .await
        .is_ok());
    }

    #[tokio::test]
    async fn account_settings_validation_failure_is_atomic() {
        let shared = tokio::sync::Mutex::new(Settings::default());
        let before = serde_json::to_value(shared.lock().await.clone()).unwrap();
        let result = save_settings_with(
            &shared,
            &json!({"language":"en", "vision_enabled":true}),
            |_| async { Err("invalid model".into()) },
            |_| panic!("must not persist"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            serde_json::to_value(shared.lock().await.clone()).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn account_settings_disk_failure_is_atomic() {
        let shared = tokio::sync::Mutex::new(Settings::default());
        let before = serde_json::to_value(shared.lock().await.clone()).unwrap();
        let result = save_settings_with(
            &shared,
            &json!({"language":"en"}),
            |_| async { Ok(()) },
            |_| Err("disk full".into()),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(
            serde_json::to_value(shared.lock().await.clone()).unwrap(),
            before
        );
    }

    #[tokio::test]
    async fn account_settings_network_validation_does_not_hold_lock() {
        let shared = tokio::sync::Mutex::new(Settings::default());
        let result = save_settings_with(
            &shared,
            &json!({"vision_enabled":true}),
            |_| async {
                assert!(
                    shared.try_lock().is_ok(),
                    "network validation holds settings lock"
                );
                Ok(())
            },
            |_| Ok(()),
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn account_settings_equal_vision_fields_do_not_validate() {
        let settings = Settings::default();
        let updates = json!({"language":"en", "vision_enabled":settings.vision_enabled,
            "vision_use_main_llm":settings.vision_use_main_llm, "vision_model":settings.vision_model,
            "vision_provider":settings.vision_provider, "vision_api_key":""});
        let shared = tokio::sync::Mutex::new(settings);
        let result = save_settings_with(
            &shared,
            &updates,
            |_| async { panic!("unchanged visual fields must not validate") },
            |_| Ok(()),
        )
        .await;
        assert!(result.is_ok());
    }

    #[test]
    fn account_settings_blank_wechat_credentials_mean_unchanged() {
        let mut settings = Settings::default();
        settings.wechat_bot_token = "bound-token".into();
        settings.wechat_gateway_token = "gateway-token".into();
        apply_settings_patch(
            &mut settings,
            &json!({"wechat_bot_token":"", "wechat_gateway_token":""}),
        );
        assert_eq!(settings.wechat_bot_token, "bound-token");
        assert_eq!(settings.wechat_gateway_token, "gateway-token");
    }

    #[test]
    fn account_settings_null_explicitly_clears_credentials() {
        let mut settings = Settings::default();
        settings.wechat_bot_token = "bound-token".into();
        settings.vision_api_key = "vision-key".into();
        settings.harmony_api_key = "main-key".into();
        apply_settings_patch(
            &mut settings,
            &json!({"wechat_bot_token":null, "vision_api_key":null, "harmony_api_key":null}),
        );
        assert!(settings.wechat_bot_token.is_empty());
        assert!(settings.vision_api_key.is_empty());
        assert!(settings.harmony_api_key.is_empty());
    }
}
