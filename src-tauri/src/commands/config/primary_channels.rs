//! Shared setup lifecycle for the two supported inbound IM channels.
//!
//! Saving a channel configuration and completing a WeChat QR login both call
//! this command. This prevents the settings page and the chat sidebar from
//! disagreeing about whether a listener has actually been started.

use crate::commands::chat::gateway::sync_gateway_channels;
use crate::commands::config::enterprise_capability::inspect_wecom_cli;
use crate::gateway::ChannelStatus;
use crate::store::AppState;
use serde::Serialize;
use tauri::State;

const WECOM: &str = "wecom";
const WECHAT: &str = "wechat";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryChannelSetupResult {
    pub channel: String,
    pub state: String,
    pub capabilities: Vec<String>,
    pub next_action: Option<String>,
    pub diagnostic: Option<String>,
}

fn result(
    channel: &str,
    state: &str,
    capabilities: Vec<String>,
    next_action: Option<String>,
    diagnostic: Option<String>,
) -> PrimaryChannelSetupResult {
    PrimaryChannelSetupResult {
        channel: channel.to_string(),
        state: state.to_string(),
        capabilities,
        next_action,
        diagnostic,
    }
}

fn is_connected(channel: &str, status: &crate::commands::chat::gateway::GatewayStatus) -> bool {
    status.channels.iter().any(|item| {
        item.name == channel && matches!(item.status, ChannelStatus::Connected)
    })
}

/// Verify any non-interactive prerequisites and start the configured channel.
/// Interactive official authorizations are intentionally returned as an action
/// for the user instead of being launched or faked by the desktop app.
#[tauri::command]
pub async fn initialize_primary_channel(
    state: State<'_, AppState>,
    channel: String,
) -> Result<PrimaryChannelSetupResult, String> {
    let channel = channel.trim().to_ascii_lowercase();
    if channel != WECOM && channel != WECHAT {
        return Err(format!("Unsupported primary channel: {channel}"));
    }

    let settings = state.settings.lock().await.clone();
    match channel.as_str() {
        WECOM => {
            if !settings.wecom_enabled
                || settings.wecom_bot_id.trim().is_empty()
                || settings.wecom_bot_secret.trim().is_empty()
            {
                return Ok(result(
                    WECOM,
                    "unconfigured",
                    Vec::new(),
                    Some("填写 Bot ID 和 Secret 并保存后，Harmony Work 会自动建立企业微信长连接。".into()),
                    None,
                ));
            }

            let cli = inspect_wecom_cli().await;
            let gateway = sync_gateway_channels(&state).await?;
            if !is_connected(WECOM, &gateway) {
                return Ok(result(
                    WECOM,
                    "repair-required",
                    Vec::new(),
                    None,
                    Some("企业微信长连接未能启动，请检查 Bot ID、Secret 和网络后重试。".into()),
                ));
            }

            let capabilities = if cli.authorized {
                vec!["contact", "doc", "meeting", "msg", "schedule", "todo"]
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            };
            let next_action = if cli.authorized {
                None
            } else if cli.available {
                Some("IM 已可收发消息。完成企业微信 CLI 官方授权后，可使用通讯录、文档、日程等企业工具。".into())
            } else {
                Some("IM 已可收发消息。安装或修复内置企业微信 CLI 后，可使用通讯录、文档、日程等企业工具。".into())
            };
            let diagnostic = (!cli.diagnostics.is_empty()).then(|| cli.diagnostics.join("\n"));
            Ok(result(WECOM, "connected", capabilities, next_action, diagnostic))
        }
        WECHAT => {
            if !settings.wechat_enabled || settings.wechat_bot_token.trim().is_empty() {
                return Ok(result(
                    WECHAT,
                    "unconfigured",
                    Vec::new(),
                    Some("点击“绑定微信”并完成扫码授权后，Harmony Work 会自动启动微信监听。".into()),
                    None,
                ));
            }
            let gateway = sync_gateway_channels(&state).await?;
            if is_connected(WECHAT, &gateway) {
                Ok(result(
                    WECHAT,
                    "connected",
                    Vec::new(),
                    None,
                    Some("微信 iLink 监听已启动。".into()),
                ))
            } else {
                Ok(result(
                    WECHAT,
                    "repair-required",
                    Vec::new(),
                    None,
                    Some("微信监听未能启动，请重新绑定微信或检查本机端口配置。".into()),
                ))
            }
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::{result, PrimaryChannelSetupResult};

    #[test]
    fn serializes_the_primary_channel_contract_in_camel_case() {
        let value: serde_json::Value = serde_json::to_value(result(
            "wecom",
            "connected",
            vec!["contact".into()],
            Some("authorize cli".into()),
            None,
        ))
        .expect("serialize setup result");
        assert_eq!(value["nextAction"], "authorize cli");
        assert_eq!(value["capabilities"][0], "contact");
        assert!(value.get("next_action").is_none());
    }

    #[test]
    fn result_preserves_channel_and_state() {
        let value: PrimaryChannelSetupResult = result("wechat", "unconfigured", vec![], None, None);
        assert_eq!(value.channel, "wechat");
        assert_eq!(value.state, "unconfigured");
    }
}
