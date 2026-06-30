use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use keyring::Entry;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::hash_map::DefaultHasher,
    collections::{BTreeMap, BTreeSet},
    env, fs,
    hash::{Hash, Hasher},
    io::{Cursor, ErrorKind, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, UserAttentionType, WindowEvent,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use zip::{write::SimpleFileOptions, ZipWriter};

const KEYRING_SERVICE: &str = "com.harmony.xiaoheng-workbuddy";
const KEYRING_USER: &str = "llm-api-key";
const OPENPISCI_MARKETPLACE_ROOT: &str = "Y:/openpisci-0.8.64/marketplace";
const TASK_MARKER: &str = "XIAOHENG_TASK_UPDATES:";
const WORK_POOL_MARKER: &str = "XIAOHENG_WORK_POOL_UPDATES:";
const RUN_MARKER: &str = "XIAOHENG_RUN_TRACE:";
const HARMONY_MARKET_URL: &str = "http://61.150.31.150:8787";
const WEIXIN_ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const WEIXIN_ILINK_APP_ID: &str = "bot";
const WEIXIN_ILINK_CLIENT_VERSION: &str = "132099";
const WEIXIN_DEFAULT_BOT_TYPE: &str = "3";
const DEFAULT_OBJECT_STORAGE_ENDPOINT: &str = "http://61.150.31.150:8788";
const DEFAULT_OBJECT_STORAGE_UPLOAD_ENDPOINT: &str = "http://192.168.11.29:8788";
const DEFAULT_OBJECT_STORAGE_BUCKET: &str = "workbuddy";
const DEFAULT_OBJECT_STORAGE_ACCESS_KEY: &str = "workbuddy-client";
const DEFAULT_OBJECT_STORAGE_SECRET_KEY: &str = "CN74np5NxcY7LlEYRGGdXvUElTBfF5192qRMzI52";
const DEFAULT_OBJECT_STORAGE_REGION: &str = "us-east-1";

static API_KEY_CACHE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
static IM_STATE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
static IM_CONFIG: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();
static WEIXIN_LOGINS: OnceLock<Mutex<BTreeMap<String, WeixinLoginSession>>> = OnceLock::new();
static WEIXIN_MONITORS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
static STOPPED_RUNS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
static HEARTBEAT_SCHEDULER_STARTED: OnceLock<()> = OnceLock::new();
static WINDOWS_PATH_RE: OnceLock<Regex> = OnceLock::new();

type HmacSha256 = Hmac<Sha256>;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn hide_command_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn configured_proxy_url() -> Option<String> {
    let env_proxy = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .filter_map(|name| env::var(name).ok())
    .map(|value| value.trim().to_owned())
    .find(|value| !value.is_empty());
    if env_proxy.is_some() {
        return env_proxy;
    }
    #[cfg(windows)]
    {
        let output = Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                "ProxyServer",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let raw = text
            .lines()
            .find(|line| line.contains("ProxyServer"))
            .and_then(|line| line.split_whitespace().last())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let proxy = raw
            .split(';')
            .find_map(|part| {
                part.strip_prefix("https=")
                    .or_else(|| part.strip_prefix("http="))
            })
            .unwrap_or(raw)
            .trim();
        if proxy.starts_with("http://") || proxy.starts_with("https://") {
            Some(proxy.to_owned())
        } else {
            Some(format!("http://{proxy}"))
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn apply_configured_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    if let Some(proxy_url) = configured_proxy_url() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            return builder.proxy(proxy);
        }
    }
    builder
}

fn model_client_builder(timeout_seconds: u64) -> reqwest::ClientBuilder {
    apply_configured_proxy(reqwest::Client::builder())
        .timeout(Duration::from_secs(timeout_seconds.max(60)))
        .connect_timeout(Duration::from_secs(20))
        .tcp_keepalive(Some(Duration::from_secs(30)))
        .pool_idle_timeout(Some(Duration::from_secs(10)))
        .http1_only()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatMessage {
    role: String,
    content: String,
    #[serde(default)]
    attachments: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentItem {
    id: String,
    name: String,
    mime: String,
    byte_size: u64,
    sha256: String,
    original_path: String,
    cache_path: String,
    created_at: String,
    source: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    provider: String,
    model: String,
    base_url: String,
    #[serde(default)]
    workspace_root: String,
    #[serde(default)]
    available_models: Vec<Value>,
    #[serde(default)]
    enabled_models: Vec<String>,
    #[serde(default = "default_agent_max_iterations")]
    agent_max_iterations: u32,
    #[serde(default = "default_max_output_tokens")]
    max_output_tokens: u32,
    #[serde(default = "default_tool_result_token_limit")]
    tool_result_token_limit: u32,
    #[serde(default = "default_llm_timeout_seconds")]
    llm_timeout_seconds: u64,
    #[serde(default)]
    allow_outside_workspace: bool,
    #[serde(default = "default_confirm_sensitive_tools")]
    confirm_shell: bool,
    #[serde(default = "default_confirm_sensitive_tools")]
    confirm_file_write: bool,
}

fn default_agent_max_iterations() -> u32 {
    50
}

fn default_max_output_tokens() -> u32 {
    4096
}

fn default_tool_result_token_limit() -> u32 {
    1200
}

fn default_llm_timeout_seconds() -> u64 {
    120
}

fn default_confirm_sensitive_tools() -> bool {
    true
}

fn observe_security_settings(settings: &AppSettings) {
    let _ = (
        settings.workspace_root.as_str(),
        settings.allow_outside_workspace,
        settings.confirm_shell,
        settings.confirm_file_write,
    );
}

#[derive(Debug, Serialize, Clone)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Value>,
}

const MAX_INITIAL_CHAT_MESSAGES: usize = 4;
const MAX_CHAT_MESSAGE_CHARS: usize = 1600;
const MAX_ASSISTANT_TOOL_ARG_CHARS: usize = 600;
const MAX_TOOL_RESULT_CHARS_FOR_MODEL: usize = 1600;
const MAX_TOOL_RESULT_CHARS_FOR_UI: usize = 12000;
const MAX_PAYLOAD_MESSAGE_CHARS: usize = 3800;
const READ_TEXT_FILE_CONTENT_LIMIT: usize = 30000;
const TEXT_PREVIEW_CHARS: usize = 1200;
const SHELL_OUTPUT_LIMIT: usize = 20000;
const HARD_MAX_AGENT_ITERATIONS: u32 = 160;
const SOFT_FINAL_MIN_ITERATIONS: u32 = 6;
const SOFT_FINAL_MAX_ITERATIONS: u32 = 60;
const MAX_REPEAT_TOOL_CALLS: u32 = 2;

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut output = value.chars().take(limit).collect::<String>();
    if value.chars().count() > limit {
        output.push_str("\n...[truncated]");
    }
    output
}

fn first_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

fn last_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars().rev().take(limit).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

fn compact_chat_content(content: &str) -> String {
    truncate_chars(content, MAX_CHAT_MESSAGE_CHARS)
}

fn chat_message_content_for_model(message: &ChatMessage) -> String {
    let mut content = compact_chat_content(&message.content);
    if !message.attachments.is_empty() {
        let details = message
            .attachments
            .iter()
            .take(8)
            .enumerate()
            .map(|(index, item)| {
                let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                let name = item.get("name").and_then(Value::as_str).unwrap_or("attachment");
                let mime = item.get("mime").and_then(Value::as_str).unwrap_or("application/octet-stream");
                let byte_size = item.get("byteSize").and_then(Value::as_u64).unwrap_or(0);
                let cache_path = item.get("cachePath").and_then(Value::as_str).unwrap_or("");
                format!(
                    "{}. {name}\n- attachmentId: {id}\n- mime: {mime}\n- byteSize: {byte_size}\n- cachePath: {cache_path}",
                    index + 1
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        content.push_str("\n\n[用户上传附件]\n这些附件已经复制到 WorkBuddy 附件缓存。需要读取文本内容时，优先调用 read_attachment_text 并传 attachmentId。\n");
        content.push_str(&details);
    }
    content
}

fn initial_openai_messages(system: &str, messages: &[ChatMessage]) -> Vec<OpenAiMessage> {
    let mut payload_messages = vec![OpenAiMessage {
        role: "system".to_owned(),
        content: Some(system.to_owned()),
        tool_call_id: None,
        tool_calls: None,
    }];
    let start = messages.len().saturating_sub(MAX_INITIAL_CHAT_MESSAGES);
    payload_messages.extend(messages[start..].iter().map(|message| OpenAiMessage {
        role: message.role.clone(),
        content: Some(chat_message_content_for_model(message)),
        tool_call_id: None,
        tool_calls: None,
    }));
    payload_messages
}

fn compact_tool_call_for_model(call: &Value) -> Value {
    let mut compact = call.clone();
    if let Some(arguments) = compact["function"]["arguments"].as_str() {
        compact["function"]["arguments"] =
            json!(truncate_chars(arguments, MAX_ASSISTANT_TOOL_ARG_CHARS));
    }
    compact
}

fn compact_tool_calls_for_model(tool_calls: &Value) -> Value {
    tool_calls
        .as_array()
        .map(|calls| Value::Array(calls.iter().map(compact_tool_call_for_model).collect()))
        .unwrap_or_else(|| tool_calls.clone())
}

fn compact_tool_result_for_model(name: &str, result: &Value, limit: usize) -> String {
    let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let mut compact = json!({
        "ok": ok,
        "tool": name
    });
    for key in [
        "path",
        "exists",
        "count",
        "source",
        "status",
        "error",
        "messageId",
        "byteSize",
        "charCount",
        "contentReturnedChars",
        "contentTruncated",
        "stdoutChars",
        "stderrChars",
        "stdoutReturnedChars",
        "stderrReturnedChars",
        "stdoutTruncated",
        "stderrTruncated",
        "returnedCount",
        "entriesTruncated",
        "extensionCounts",
        "verificationNote",
    ] {
        if let Some(value) = result.get(key) {
            compact[key] = value.clone();
        }
    }
    if let Some(entries) = result.get("entries").and_then(Value::as_array) {
        compact["entries"] = json!(entries.iter().take(40).cloned().collect::<Vec<_>>());
        compact["entryCount"] = json!(entries.len());
    }
    if let Some(results) = result.get("results").and_then(Value::as_array) {
        compact["results"] = json!(results
            .iter()
            .take(8)
            .map(|item| {
                let mut compact_item = item.clone();
                if let Some(content) = compact_item.get("content").and_then(Value::as_str) {
                    compact_item["content"] = json!(truncate_chars(content, limit));
                }
                compact_item
            })
            .collect::<Vec<_>>());
        compact["resultCount"] = json!(results.len());
    }
    if let Some(rows) = result.get("rows").and_then(Value::as_array) {
        compact["rowPreview"] = json!(rows.iter().take(8).cloned().collect::<Vec<_>>());
        compact["rowCount"] = json!(rows.len());
    }
    if let Some(stdout) = result.get("stdout").and_then(Value::as_str) {
        compact["stdout"] = json!(truncate_chars(stdout, limit / 2));
    }
    if let Some(stdout_preview) = result.get("stdoutPreview").and_then(Value::as_str) {
        compact["stdoutPreview"] = json!(truncate_chars(stdout_preview, limit / 3));
    }
    if let Some(stdout_tail) = result.get("stdoutTail").and_then(Value::as_str) {
        compact["stdoutTail"] = json!(truncate_chars(stdout_tail, limit / 3));
    }
    if let Some(stderr) = result.get("stderr").and_then(Value::as_str) {
        compact["stderr"] = json!(truncate_chars(stderr, limit / 2));
    }
    if let Some(stderr_preview) = result.get("stderrPreview").and_then(Value::as_str) {
        compact["stderrPreview"] = json!(truncate_chars(stderr_preview, limit / 3));
    }
    if let Some(stderr_tail) = result.get("stderrTail").and_then(Value::as_str) {
        compact["stderrTail"] = json!(truncate_chars(stderr_tail, limit / 3));
    }
    if let Some(content) = result.get("content").and_then(Value::as_str) {
        compact["content"] = json!(truncate_chars(content, limit / 2));
    }
    if let Some(content_preview) = result.get("contentPreview").and_then(Value::as_str) {
        compact["contentPreview"] = json!(truncate_chars(content_preview, limit / 3));
    }
    if let Some(content_tail) = result.get("contentTail").and_then(Value::as_str) {
        compact["contentTail"] = json!(truncate_chars(content_tail, limit / 3));
    }
    let text = serde_json::to_string(&compact).unwrap_or_else(|_| result.to_string());
    truncate_chars(&text, limit)
}

fn compact_tool_step_for_ui(
    id: &str,
    name: &str,
    args: &Value,
    result: &Value,
    limit: usize,
) -> Value {
    json!({
        "id": id,
        "name": name,
        "input": args,
        "result": truncate_chars(&result.to_string(), limit),
        "isError": result.get("ok").and_then(Value::as_bool) == Some(false),
        "completed": true
    })
}

fn effective_agent_max_iterations(settings: &AppSettings) -> u32 {
    let floor = if settings.provider == "siliconflow" {
        64
    } else {
        24
    };
    settings
        .agent_max_iterations
        .max(floor)
        .clamp(1, HARD_MAX_AGENT_ITERATIONS)
}

fn soft_final_iteration(max_iterations: u32) -> u32 {
    ((max_iterations as f32 * 0.70).ceil() as u32).clamp(
        SOFT_FINAL_MIN_ITERATIONS,
        SOFT_FINAL_MAX_ITERATIONS.min(max_iterations),
    )
}

fn final_summary_message(reason: &str) -> OpenAiMessage {
    OpenAiMessage {
        role: "system".to_owned(),
        content: Some(format!(
            "{reason}\nStop calling tools now. Produce the final user-facing answer in Simplified Chinese. If the user requested a file artifact, report the exact path only if it has been created and verified; otherwise say the artifact is incomplete and name the blocking reason. Summarize completed work, created/modified file paths, remaining risks, and task status."
        )),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn tool_result_is_error(step: &Value) -> bool {
    step.get("isError").and_then(Value::as_bool) == Some(true)
}

fn tool_step_name(step: &Value) -> &str {
    step.get("name").and_then(Value::as_str).unwrap_or_default()
}

fn tool_result_text(step: &Value) -> String {
    step.get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase()
}

fn has_verified_artifact_evidence(tool_steps: &[Value]) -> bool {
    tool_steps.iter().any(|step| {
        if tool_result_is_error(step) {
            return false;
        }
        let name = tool_step_name(step);
        let result = tool_result_text(step);
        matches!(name, "news_to_excel" | "write_text_file")
            || (name == "shell_exec"
                && result.contains("\"ok\":true")
                && (result.contains(".xlsx")
                    || result.contains(".pptx")
                    || result.contains(".docx")
                    || result.contains(".pdf")
                    || result.contains(".md")
                    || result.contains(".csv")
                    || result.contains(".txt")))
            || (name == "list_files"
                && result.contains("\"entries\"")
                && (result.contains(".xlsx")
                    || result.contains(".pptx")
                    || result.contains(".docx")
                    || result.contains(".pdf")
                    || result.contains(".md")
                    || result.contains(".csv")
                    || result.contains(".txt")))
            || (name == "read_text_file" && result.contains("\"exists\":true"))
    })
}

fn has_state_change_evidence(tool_steps: &[Value]) -> bool {
    tool_steps.iter().any(|step| {
        if tool_result_is_error(step) {
            return false;
        }
        matches!(
            tool_step_name(step),
            "task_plan_update"
                | "work_pool_update"
                | "memory_append"
                | "computer_control"
                | "browser_control"
                | "shell_exec"
        )
    })
}

fn tool_results_have_truncation(tool_steps: &[Value]) -> bool {
    tool_steps.iter().any(|step| {
        let result = tool_result_text(step);
        result.contains("\"stdouttruncated\":true")
            || result.contains("\"stderrtruncated\":true")
            || result.contains("\"contenttruncated\":true")
            || result.contains("\"entriestruncated\":true")
            || result.contains("...[truncated]")
    })
}

fn should_force_final(
    prompt: &str,
    iteration: u32,
    force_final_after: u32,
    max_iterations: u32,
    tool_steps: &[Value],
) -> bool {
    if tool_steps.is_empty() || iteration < force_final_after {
        return false;
    }
    if iteration + 6 >= max_iterations {
        return true;
    }
    if asks_artifact_generation(prompt) {
        return has_verified_artifact_evidence(tool_steps);
    }
    if prompt_requires_real_tool_execution(prompt) {
        return has_state_change_evidence(tool_steps) && !tool_results_have_truncation(tool_steps);
    }
    true
}

fn tool_call_fingerprint(name: &str, args: &Value) -> String {
    format!("{name}:{}", truncate_chars(&args.to_string(), 500))
}

fn repeat_limit_for_tool(name: &str) -> u32 {
    match name {
        "read_text_file"
        | "read_attachment_text"
        | "list_files"
        | "team_roster"
        | "skill_search"
        | "memory_search" => 8,
        _ => MAX_REPEAT_TOOL_CALLS,
    }
}

fn mutates_tool_observable_state(name: &str, result: &Value) -> bool {
    result.get("ok").and_then(Value::as_bool) == Some(true)
        && matches!(
            name,
            "write_text_file"
                | "shell_exec"
                | "task_plan_update"
                | "work_pool_update"
                | "memory_append"
                | "news_to_excel"
                | "computer_control"
        )
}

async fn send_openai_payload(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    settings: &AppSettings,
    mut payload: Value,
) -> Result<(reqwest::Response, usize), String> {
    let payload_bytes = serde_json::to_vec(&payload)
        .map(|value| value.len())
        .unwrap_or_default();
    let send_once = |payload: Value| {
        client
            .post(endpoint)
            .bearer_auth(api_key)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("accept-encoding", "identity")
            .header("cache-control", "no-cache")
            .json(&payload)
            .send()
    };
    match send_once(payload.clone()).await {
        Ok(response) => Ok((response, payload_bytes)),
        Err(first_error) if payload.get("tools").is_some() => {
            payload.as_object_mut().map(|object| {
                object.remove("tools");
                object.remove("tool_choice");
            });
            let retry_bytes = serde_json::to_vec(&payload).map(|value| value.len()).unwrap_or_default();
            send_once(payload).await.map(|response| (response, retry_bytes)).map_err(|retry_error| {
                format!(
                    "failed to connect model service (provider={}, model={}, url={}, payload={} bytes; no-tools retry payload={} bytes): first={first_error}; retry={retry_error}",
                    settings.provider, settings.model, endpoint, payload_bytes, retry_bytes
                )
            })
        }
        Err(error) => Err(format!(
            "failed to connect model service (provider={}, model={}, url={}, payload={} bytes): {error}",
            settings.provider, settings.model, endpoint, payload_bytes
        )),
    }
}

fn estimate_openai_message_chars(message: &OpenAiMessage) -> usize {
    serde_json::to_string(message)
        .map(|value| value.chars().count())
        .unwrap_or(0)
}

fn shrink_payload_messages(messages: &[OpenAiMessage], budget: usize) -> Vec<OpenAiMessage> {
    if messages.len() <= 2 {
        return messages.to_vec();
    }
    let total: usize = messages.iter().map(estimate_openai_message_chars).sum();
    if total <= budget {
        return messages.to_vec();
    }

    let system = messages.first().cloned();
    let mut cut = messages.len();
    let mut used = 0usize;
    while cut > 1 {
        let candidate = &messages[cut - 1];
        let next_used = used.saturating_add(estimate_openai_message_chars(candidate));
        if next_used > budget {
            break;
        }
        used = next_used;
        cut -= 1;
    }

    while cut < messages.len() && cut > 1 && messages[cut].role == "tool" {
        cut -= 1;
    }

    let mut compact = Vec::new();
    if let Some(system) = system {
        compact.push(system);
    }
    if cut > 1 {
        compact.push(OpenAiMessage {
            role: "assistant".to_owned(),
            content: Some(format!(
                "Earlier conversation and tool details were compacted locally to keep the provider request stable. Retained the latest {} messages.",
                messages.len().saturating_sub(cut)
            )),
            tool_call_id: None,
            tool_calls: None,
        });
    }
    compact.extend(messages[cut..].iter().cloned());
    compact
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MarketItem {
    id: String,
    name: String,
    description: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    trusted: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    featured: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
struct MarketplaceIndex {
    #[serde(default)]
    experts: Vec<MarketItem>,
    #[serde(default)]
    teams: Vec<MarketItem>,
    #[serde(default)]
    skills: Vec<MarketItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallRequest {
    item: MarketItem,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillMarketSearchRequest {
    source: String,
    query: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistrySkillInstallRequest {
    source: String,
    slug: String,
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImConfigRequest {
    channel_id: String,
    config: Value,
}

#[derive(Debug, Clone)]
struct WeixinLoginSession {
    qrcode: String,
    current_base_url: String,
    started_ms: u128,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillLocatorInstallRequest {
    locator: String,
    name: Option<String>,
}

#[derive(Debug, Clone)]
struct ObjectStorageConfig {
    enabled: bool,
    endpoint: String,
    upload_endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
    link_expiry_hours: u64,
}

fn now_string() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    format!("{seconds}")
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create app data dir: {e}"))?;
    Ok(dir)
}

fn data_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("workspace-state.json"))
}

fn memory_file(app: &AppHandle) -> Result<PathBuf, String> {
    let path = data_dir(app)?.join("MEMORY.md");
    if !path.exists() {
        fs::write(
            &path,
            "# Xiaoheng WorkBuddy Memory\n\n长期有效的用户偏好、项目事实、决策、承诺和复用规则会沉淀在这里。\n\n",
        )
        .map_err(|e| format!("failed to initialize MEMORY.md: {e}"))?;
    }
    Ok(path)
}

fn memory_index_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("memory-index.json"))
}

fn stable_hash(value: &str) -> String {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn append_task_updates(answer: &str, updates: &[Value]) -> String {
    if updates.is_empty() {
        return answer.to_owned();
    }
    format!(
        "{answer}\n\n<!-- {TASK_MARKER}{} -->",
        serde_json::to_string(updates).unwrap_or_else(|_| "[]".to_owned())
    )
}

fn append_work_pool_updates(answer: &str, updates: &Value) -> String {
    if updates.is_null()
        || updates
            .as_object()
            .map(|value| value.is_empty())
            .unwrap_or(false)
    {
        return answer.to_owned();
    }
    format!(
        "{answer}\n\n<!-- {WORK_POOL_MARKER}{} -->",
        serde_json::to_string(updates).unwrap_or_else(|_| "{}".to_owned())
    )
}

fn append_run_trace(answer: &str, todos: &[Value], tool_steps: &[Value]) -> String {
    if todos.is_empty() && tool_steps.is_empty() {
        return answer.to_owned();
    }
    format!(
        "{answer}\n\n<!-- {RUN_MARKER}{} -->",
        serde_json::to_string(&json!({"todos": todos, "toolSteps": tool_steps}))
            .unwrap_or_else(|_| "{}".to_owned())
    )
}

fn append_hidden_updates(
    answer: &str,
    tasks: &[Value],
    work_pool_updates: &Value,
    todos: &[Value],
    tool_steps: &[Value],
) -> String {
    let with_tasks = append_task_updates(answer, tasks);
    let with_work_pool = append_work_pool_updates(&with_tasks, work_pool_updates);
    append_run_trace(&with_work_pool, todos, tool_steps)
}

fn claims_completed_artifact_without_evidence(answer: &str) -> bool {
    let text = answer.to_lowercase();
    [
        "已生成",
        "成功生成",
        "已保存",
        "保存到",
        "已创建",
        "已写入",
        "已执行",
        "任务已执行",
        "已完成步骤",
        "截图记录",
        "抓取到",
        "图片 url",
        "image url",
        "generated",
        "saved",
        "created",
    ]
    .iter()
    .any(|needle| text.contains(&needle.to_lowercase()))
}

fn answer_requests_permission(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    answer.contains("权限")
        || answer.contains("完全访问")
        || answer.contains("请求批准")
        || answer.contains("受限")
        || lower.contains("permission")
        || lower.contains("approval")
}

fn unverified_execution_block(prompt: &str, answer: &str, permission_mode: &str) -> bool {
    if permission_mode != "full" && answer_requests_permission(answer) {
        return false;
    }
    prompt_requires_real_tool_execution(prompt)
}

fn guard_unverified_completion(answer: &str, tool_steps: &[Value]) -> String {
    if tool_steps.is_empty() && claims_completed_artifact_without_evidence(answer) {
        format!(
            "本轮没有产生任何真实工具调用记录，因此不能声称任务已执行或文件已生成。\n\n模型原始回复已被 Gateway 拦截：{}\n\n请重新发起任务，Gateway 会先选择匹配技能并通过工具实际执行、保存和验证结果。",
            truncate_chars(answer.trim(), 800)
        )
    } else {
        answer.to_owned()
    }
}

fn guard_completion_for_prompt(prompt: &str, answer: &str, tool_steps: &[Value]) -> String {
    let guarded = guard_unverified_completion(answer, tool_steps);
    if asks_artifact_generation(prompt)
        && claims_completed_artifact_without_evidence(&guarded)
        && !has_verified_artifact_evidence(tool_steps)
    {
        return format!(
            "Gateway delivery validation failed: the reply claims completion or file generation, but tool records do not contain verified artifact evidence such as path, existence, size, page/row count, or content verification.\n\nThis task cannot be marked complete yet. Continue generation and verification, or state the blocking reason.\n\nOriginal reply summary: {}",
            truncate_chars(guarded.trim(), 900)
        );
    }
    if prompt_requires_real_tool_execution(prompt)
        && claims_completed_artifact_without_evidence(&guarded)
        && !has_state_change_evidence(tool_steps)
    {
        return format!(
            "Gateway execution validation failed: the reply claims execution or state changes, but tool records do not contain reliable execution evidence.\n\nThis task cannot be marked complete yet. Continue real execution, or state the blocking reason.\n\nOriginal reply summary: {}",
            truncate_chars(guarded.trim(), 900)
        );
    }
    guarded
}

fn tool_execution_correction_message(prompt: &str, answer: &str) -> OpenAiMessage {
    OpenAiMessage {
        role: "user".to_owned(),
        content: Some(format!(
            "Gateway 校验失败：用户请求需要真实工具执行/文件生成/状态变更，但上一轮 assistant 没有调用任何工具，只输出了说明文字。\n\
原始用户请求：{}\n\
上一轮回复摘要：{}\n\
现在必须继续执行，不要再解释能力、不要询问用户想用哪个技能。请立即选择最少必要工具完成任务；如果是文件制品，写入/执行/验证真实文件；如果确实被权限阻止，只能明确请求用户提升权限。",
            truncate_chars(prompt.trim(), 800),
            truncate_chars(answer.trim(), 800)
        )),
        tool_call_id: None,
        tool_calls: None,
    }
}

fn blocked_unverified_execution_answer(prompt: &str, answer: &str) -> String {
    format!(
        "Gateway 已拦截本轮回复：这条请求需要真实执行，但模型连续没有调用工具。\n\n用户请求：{}\n\n被拦截的模型回复摘要：{}\n\n请重试，或切换到更稳定支持 tool_calls 的模型；WorkBuddy 不会把未执行的说明当作完成结果。",
        truncate_chars(prompt.trim(), 500),
        truncate_chars(answer.trim(), 500)
    )
}

fn strip_hidden_agent_markers(answer: &str) -> String {
    let mut text = answer.to_owned();
    for marker in [TASK_MARKER, WORK_POOL_MARKER, RUN_MARKER] {
        loop {
            let needle = format!("<!-- {marker}");
            let Some(start) = text.find(&needle) else {
                break;
            };
            let Some(end) = text[start..].find("-->") else {
                break;
            };
            text.replace_range(start..start + end + 3, "");
        }
    }
    text.trim().to_owned()
}

fn xml_unescape(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn extract_tag_value(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(xml_unescape(block[start..end].trim()))
}

fn extract_quoted_attribute(block: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = block.find(&needle)? + needle.len();
    let end = block[start..].find('"')? + start;
    Some(xml_unescape(block[start..end].trim()))
}

fn extract_dsml_parameter(block: &str, name: &str) -> Option<String> {
    let normalized = normalize_pseudo_tool_markup(block);
    let mut rest = normalized.as_str();
    while let Some(start) = rest.find("<｜DSML｜parameter") {
        let after_start = &rest[start..];
        let tag_end = after_start.find('>')?;
        let open_tag = &after_start[..tag_end];
        let body_start = tag_end + 1;
        let body = &after_start[body_start..];
        let end = body.find("</｜DSML｜parameter>")?;
        if extract_quoted_attribute(open_tag, "name").as_deref() == Some(name) {
            return Some(xml_unescape(body[..end].trim()));
        }
        rest = &body[end + "</｜DSML｜parameter>".len()..];
    }
    None
}

fn dsml_tool_calls_from_text(content: &str) -> Vec<Value> {
    let mut calls = Vec::new();
    let normalized = normalize_pseudo_tool_markup(content);
    let mut rest = normalized.as_str();
    while let Some(start) = rest.find("<｜DSML｜invoke") {
        let after_start = &rest[start..];
        let Some(tag_end) = after_start.find('>') else {
            break;
        };
        let open_tag = &after_start[..tag_end];
        let Some(name) =
            extract_quoted_attribute(open_tag, "name").filter(|value| !value.is_empty())
        else {
            rest = &after_start[tag_end + 1..];
            continue;
        };
        let body = &after_start[tag_end + 1..];
        let Some(end) = body.find("</｜DSML｜invoke>") else {
            break;
        };
        let block = &body[..end];
        let arguments =
            extract_dsml_parameter(block, "arguments").unwrap_or_else(|| "{}".to_owned());
        calls.push(json!({
            "id": format!("pseudo-dsml-tool-{}", calls.len() + 1),
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments
            }
        }));
        rest = &body[end + "</｜DSML｜invoke>".len()..];
    }
    calls
}

fn clean_pseudo_tool_name(raw_name: &str) -> String {
    let trimmed = raw_name.trim();
    let name = trimmed
        .strip_prefix("<name>")
        .and_then(|value| value.strip_suffix("</name>"))
        .unwrap_or(trimmed)
        .trim();
    name.split('<')
        .next()
        .unwrap_or(name)
        .trim()
        .trim_matches(|ch: char| ch == '`' || ch == '"' || ch == '\'')
        .to_owned()
}

fn pseudo_tool_call_from_json_block(block: &str, index: usize) -> Option<Value> {
    let cleaned = block
        .trim()
        .trim_matches('`')
        .trim_start_matches("json")
        .trim();
    let value: Value = serde_json::from_str(cleaned).ok()?;
    let function = value.get("function").unwrap_or(&value);
    let name = function
        .get("name")
        .or_else(|| value.get("name"))
        .or_else(|| value.get("tool"))
        .and_then(Value::as_str)
        .map(clean_pseudo_tool_name)
        .filter(|name| !name.is_empty())?;
    let arguments = function
        .get("arguments")
        .or_else(|| value.get("arguments"))
        .or_else(|| value.get("args"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arguments = if let Some(text) = arguments.as_str() {
        text.to_owned()
    } else {
        arguments.to_string()
    };
    Some(json!({
        "id": format!("pseudo-json-tool-{}", index),
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments
        }
    }))
}

fn pseudo_tool_calls_from_text(content: &str) -> Vec<Value> {
    let mut calls = Vec::new();
    let normalized = normalize_pseudo_tool_markup(content);
    let mut rest = normalized.as_str();
    while let Some(start) = rest.find("<tool_call>") {
        let after_start = &rest[start + "<tool_call>".len()..];
        let end = after_start
            .find("</tool_call>")
            .unwrap_or(after_start.len());
        let block = &after_start[..end];
        if let Some(call) = pseudo_tool_call_from_json_block(block, calls.len() + 1) {
            calls.push(call);
            rest = if end < after_start.len() {
                &after_start[end + "</tool_call>".len()..]
            } else {
                ""
            };
            continue;
        }
        let raw_name = block
            .lines()
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let name = clean_pseudo_tool_name(raw_name);
        if !name.is_empty() {
            let mut args = serde_json::Map::new();
            let mut search = block;
            while let Some(key_pos) = search.find("<arg_key>") {
                let key_start = key_pos + "<arg_key>".len();
                let Some(key_end_rel) = search[key_start..].find("</arg_key>") else {
                    break;
                };
                let key_end = key_start + key_end_rel;
                let key = xml_unescape(search[key_start..key_end].trim());
                let after_key = &search[key_end + "</arg_key>".len()..];
                let Some(value) = extract_tag_value(after_key, "arg_value") else {
                    break;
                };
                args.insert(key, json!(value));
                search = after_key
                    .find("</arg_value>")
                    .map(|idx| &after_key[idx + "</arg_value>".len()..])
                    .unwrap_or("");
            }
            calls.push(json!({
                "id": format!("pseudo-tool-{}", calls.len() + 1),
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": Value::Object(args).to_string()
                }
            }));
        }
        rest = if end < after_start.len() {
            &after_start[end + "</tool_call>".len()..]
        } else {
            ""
        };
    }
    calls.extend(dsml_tool_calls_from_text(&normalized));
    calls
}

fn normalize_pseudo_tool_markup(content: &str) -> String {
    content
        .replace("<｜｜DSML｜｜", "<｜DSML｜")
        .replace("</｜｜DSML｜｜", "</｜DSML｜")
        .replace("<||DSML||", "<｜DSML｜")
        .replace("</||DSML||", "</｜DSML｜")
}

fn contains_pseudo_tool_call(content: &str) -> bool {
    let normalized = normalize_pseudo_tool_markup(content);
    normalized.contains("<tool_call>")
        || normalized.contains("</tool_call>")
        || normalized.contains("<｜DSML｜tool_calls")
        || normalized.contains("<｜DSML｜invoke")
}

fn remove_tagged_blocks(mut output: String, open: &str, close: &str) -> String {
    loop {
        let Some(start) = output.find(open) else {
            break;
        };
        let Some(end_rel) = output[start..].find(close) else {
            output.replace_range(start..output.len(), "");
            break;
        };
        let end = start + end_rel + close.len();
        output.replace_range(start..end, "");
    }
    output
}

fn remove_pseudo_tool_call_blocks(content: &str) -> String {
    let mut output = normalize_pseudo_tool_markup(content);
    output = remove_tagged_blocks(output, "<tool_call>", "</tool_call>");
    output = remove_tagged_blocks(output, "<｜DSML｜tool_calls", "</｜DSML｜tool_calls>");
    output = remove_tagged_blocks(output, "<｜DSML｜invoke", "</｜DSML｜invoke>");
    output.trim().to_owned()
}

fn pseudo_tool_call_parse_error() -> String {
    "模型输出了伪工具调用文本，但未返回标准 tool_calls，且 WorkBuddy 未能安全解析。已拦截这段协议文本，避免把它当作普通回复展示；请重试、切换支持工具调用的模型，或关闭 token 流式输出后再试。".to_owned()
}

fn extract_loose_command_arg(input: &str) -> Option<String> {
    let key_pos = input.find("\"command\"")?;
    let after_key = &input[key_pos + "\"command\"".len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let value = after_colon.strip_prefix('"')?;
    for delimiter in ["\", \"cwd\"", "\",\"cwd\"", "\", \"shell\"", "\",\"shell\""] {
        if let Some(end) = value.find(delimiter) {
            return Some(value[..end].to_owned());
        }
    }
    let end = value.rfind('"')?;
    Some(value[..end].to_owned())
}

fn extract_loose_string_arg(input: &str, key: &str) -> Option<String> {
    let key_literal = format!("\"{key}\"");
    let key_pos = input.find(&key_literal)?;
    let after_key = &input[key_pos + key_literal.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let value = after_colon.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_owned())
}

fn parse_tool_arguments(args_text: &str) -> Value {
    if let Ok(value) = serde_json::from_str(args_text) {
        return value;
    }
    let Some(command) = extract_loose_command_arg(args_text) else {
        return json!({});
    };
    let mut args = serde_json::Map::new();
    args.insert("command".to_owned(), json!(command));
    if let Some(cwd) = extract_loose_string_arg(args_text, "cwd") {
        args.insert("cwd".to_owned(), json!(cwd));
    }
    if let Some(shell) = extract_loose_string_arg(args_text, "shell") {
        args.insert("shell".to_owned(), json!(shell));
    }
    Value::Object(args)
}

fn emit_stream(app: &AppHandle, run_id: &str, event_type: &str, mut payload: Value) {
    payload["type"] = json!(event_type);
    payload["runId"] = json!(run_id);
    let _ = app.emit("xiaoheng-chat-stream", payload);
}

fn run_stop_requested(run_id: &str) -> bool {
    STOPPED_RUNS
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
        .map(|runs| runs.contains(run_id))
        .unwrap_or(false)
}

fn clear_run_stop(run_id: &str) {
    if let Ok(mut runs) = STOPPED_RUNS
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
    {
        runs.remove(run_id);
    }
}

fn emit_stopped(
    app: &AppHandle,
    run_id: &str,
    task_updates: &[Value],
    run_todos: &[Value],
    tool_steps: &[Value],
) {
    let todos: Vec<Value> = if run_todos.is_empty() {
        vec![json!({"id": "stopped", "content": "stopped", "status": "cancelled"})]
    } else {
        run_todos
            .iter()
            .map(|todo| {
                let mut item = todo.clone();
                if item.get("status").and_then(Value::as_str) != Some("completed") {
                    item["status"] = json!("cancelled");
                }
                item
            })
            .collect()
    };
    emit_stream(
        app,
        run_id,
        "stopped",
        json!({
            "content": "stopped",
            "tasks": task_updates,
            "todos": todos,
            "toolSteps": tool_steps
        }),
    );
}

fn make_todos_from_tasks(tasks: &[Value]) -> Vec<Value> {
    tasks
        .iter()
        .map(|task| {
            let status = match task.get("status").and_then(Value::as_str).unwrap_or("todo") {
                "doing" => "in_progress",
                "review" | "done" => "completed",
                "stopped" => "cancelled",
                _ => "pending",
            };
            json!({
                "id": task.get("id").cloned().unwrap_or(Value::Null),
                "content": task.get("title").cloned().unwrap_or(Value::Null),
                "status": status
            })
        })
        .collect()
}

fn api_key_fallback_file(app: &AppHandle, provider: &str) -> Result<PathBuf, String> {
    let name = if provider.trim().is_empty() {
        "api-key.local".to_owned()
    } else {
        format!("api-key-{}.local", safe_slug(provider))
    };
    Ok(data_dir(app)?.join(name))
}

fn installed_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("installed-skills");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create installed skills dir: {e}"))?;
    Ok(dir)
}

fn unified_skills_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = installed_skills_dir(app)?.join("skills");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create skills dir: {e}"))?;
    Ok(dir)
}

fn percent_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn safe_slug(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn copy_skill_dir(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| format!("failed to create skill target dir: {e}"))?;
    for entry in fs::read_dir(source).map_err(|e| format!("failed to read skill dir: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(
            name.as_str(),
            ".git"
                | ".github"
                | "__pycache__"
                | ".pytest_cache"
                | ".venv"
                | "venv"
                | "node_modules"
        ) {
            continue;
        }
        let src = entry.path();
        let dst = target.join(&name);
        if src.is_dir() {
            copy_skill_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst)
                .map_err(|e| format!("failed to copy {}: {e}", src.to_string_lossy()))?;
        }
    }
    Ok(())
}

fn unpack_zip_bytes(bytes: &[u8], target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| format!("failed to create unzip dir: {e}"))?;
    let reader = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("invalid skill zip: {e}"))?;
    let target_root = target
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(target)?;
            target.canonicalize()
        })
        .map_err(|e| format!("failed to resolve unzip dir: {e}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let Some(enclosed) = file.enclosed_name().map(PathBuf::from) else {
            continue;
        };
        let out_path = target_root.join(enclosed);
        if !out_path.starts_with(&target_root) {
            continue;
        }
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn find_skill_md(root: &Path) -> Option<PathBuf> {
    for candidate in [
        root.join("skill").join("SKILL.md"),
        root.join("SKILL.md"),
        root.join("skill.md"),
    ] {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[tauri::command]
fn load_state(app: AppHandle) -> Result<Option<Value>, String> {
    let path = data_file(&app)?;
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("failed to read local state: {e}"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("failed to parse local state: {e}"))
}

#[tauri::command]
fn save_state(app: AppHandle, state: Value) -> Result<(), String> {
    let path = data_file(&app)?;
    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&temp, text).map_err(|e| format!("failed to write local state: {e}"))?;
    fs::rename(&temp, &path).map_err(|e| format!("failed to commit local state: {e}"))
}

#[tauri::command]
fn memory_file_read(app: AppHandle) -> Result<Value, String> {
    memory_read(&app)
}

#[tauri::command]
async fn memory_file_search(
    app: AppHandle,
    query: String,
    limit: Option<u64>,
    settings: Value,
) -> Result<Value, String> {
    let client = model_client_builder(60)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    memory_search_indexed(
        &app,
        &client,
        &json!({ "settings": settings }),
        &json!({ "query": query, "limit": limit.unwrap_or(6) }),
    )
    .await
}

#[tauri::command]
async fn memory_file_append(
    app: AppHandle,
    title: Option<String>,
    content: String,
    settings: Value,
) -> Result<Value, String> {
    let result = memory_append(
        &app,
        &json!({ "title": title.unwrap_or_else(|| "记忆更新".to_owned()), "content": content }),
    )?;
    let client = model_client_builder(60)
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let index_result = rebuild_memory_index(&app, &client, &json!({ "settings": settings })).await;
    Ok(json!({
        "ok": true,
        "path": result.get("path").cloned().unwrap_or(Value::Null),
        "byteSize": result.get("byteSize").cloned().unwrap_or(Value::Null),
        "charCount": result.get("charCount").cloned().unwrap_or(Value::Null),
        "indexOk": index_result.is_ok(),
        "index": index_result.unwrap_or_else(|error| json!({"ok": false, "error": error}))
    }))
}

fn credential(provider: &str) -> Result<Entry, String> {
    let user = format!("{KEYRING_USER}-{}", safe_slug(provider));
    Entry::new(KEYRING_SERVICE, &user).map_err(|e| format!("credential store unavailable: {e}"))
}

fn cached_api_key(provider: &str) -> Option<String> {
    API_KEY_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .ok()
        .and_then(|value| {
            value
                .get(provider)
                .cloned()
                .or_else(|| value.get("compatible").cloned())
        })
        .filter(|value| !value.trim().is_empty())
}

fn set_cached_api_key(provider: &str, api_key: &str) {
    if let Ok(mut value) = API_KEY_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
    {
        value.insert(provider.to_owned(), api_key.trim().to_owned());
    }
}

fn env_api_key(provider: &str) -> Option<String> {
    let candidates: &[&str] = match provider {
        "openai" | "compatible" => &["OPENAI_API_KEY", "XIAOHENG_API_KEY", "LLM_API_KEY"],
        "anthropic" => &["ANTHROPIC_API_KEY", "XIAOHENG_API_KEY", "LLM_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY", "XIAOHENG_API_KEY", "LLM_API_KEY"],
        "siliconflow" => &["SILICONFLOW_API_KEY", "XIAOHENG_API_KEY", "LLM_API_KEY"],
        "zai" => &[
            "ZAI_API_KEY",
            "BIGMODEL_API_KEY",
            "XIAOHENG_API_KEY",
            "LLM_API_KEY",
        ],
        "qwen" => &[
            "DASHSCOPE_API_KEY",
            "QWEN_API_KEY",
            "XIAOHENG_API_KEY",
            "LLM_API_KEY",
        ],
        "doubao" => &[
            "ARK_API_KEY",
            "DOUBAO_API_KEY",
            "XIAOHENG_API_KEY",
            "LLM_API_KEY",
        ],
        "mimo" => &["MIMO_API_KEY", "XIAOHENG_API_KEY", "LLM_API_KEY"],
        _ => &["XIAOHENG_API_KEY", "LLM_API_KEY", "OPENAI_API_KEY"],
    };
    candidates
        .iter()
        .filter_map(|name| env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn provider_base_url(provider: &str, custom_base_url: &str) -> String {
    let custom = custom_base_url.trim().trim_end_matches('/');
    if !custom.is_empty() {
        return custom.to_owned();
    }
    let fallback = match provider {
        "openai" => "https://api.openai.com/v1",
        "anthropic" => "https://api.anthropic.com/v1",
        "deepseek" => "https://api.deepseek.com/v1",
        "siliconflow" => "https://api.siliconflow.cn/v1",
        "zai" => "https://open.bigmodel.cn/api/paas/v4",
        "qwen" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "doubao" => "https://ark.cn-beijing.volces.com/api/v3",
        "mimo" => "https://api.mimoai.com/v1",
        "ollama" => "http://127.0.0.1:11434/v1",
        _ => custom_base_url,
    };
    fallback.trim_end_matches('/').to_owned()
}

fn classify_model_id(id: &str) -> &'static str {
    let value = id.to_lowercase();
    if value.contains("embedding") || value.contains("embed") {
        "embedding"
    } else if value.contains("rerank") {
        "rerank"
    } else if value.contains("image")
        || value.contains("wanx")
        || value.contains("flux")
        || value.contains("stable-diffusion")
    {
        "image"
    } else if value.contains("video") || value.contains("wan2") || value.contains("sora") {
        "video"
    } else if value.contains("audio")
        || value.contains("tts")
        || value.contains("speech")
        || value.contains("whisper")
    {
        "audio"
    } else if value.contains("vision") || value.contains("vl") || value.contains("qvq") {
        "vision"
    } else if value.contains("reason") || value.contains("-r1") || value.contains("thinking") {
        "reasoning"
    } else if value.contains("coder") || value.contains("code") {
        "coder"
    } else if value.contains("math") {
        "math"
    } else if value.contains("tool") {
        "tools"
    } else if value.contains("fim") {
        "fim"
    } else if value.contains("moe") || value.contains("mixtral") {
        "moe"
    } else {
        "chat"
    }
}

fn provider_icon(provider: &str) -> &'static str {
    match provider {
        "openai" => "OpenAI",
        "anthropic" => "A",
        "deepseek" => "DS",
        "siliconflow" => "SF",
        "zai" => "ZAI",
        "mimo" => "M",
        "qwen" => "Q",
        "doubao" => "DB",
        "ollama" => "O",
        _ => "AI",
    }
}

fn normalize_model_items(provider: &str, body: &Value) -> Vec<Value> {
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| body.get("models").and_then(Value::as_array))
        .or_else(|| body.get("items").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    let mut seen = BTreeSet::new();
    data.into_iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("model"))
                .and_then(Value::as_str)?
                .trim()
                .to_owned();
            if id.is_empty() || !seen.insert(id.clone()) {
                return None;
            }
            Some(json!({
                "id": id,
                "name": item.get("display_name").or_else(|| item.get("displayName")).or_else(|| item.get("name")).and_then(Value::as_str).unwrap_or(&id),
                "provider": provider,
                "category": classify_model_id(&id),
                "ownedBy": item.get("owned_by").or_else(|| item.get("ownedBy")).and_then(Value::as_str).unwrap_or(provider),
                "icon": provider_icon(provider)
            }))
        })
        .collect()
}

fn merge_model_items(provider: &str, bodies: &[Value]) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for model in bodies
        .iter()
        .flat_map(|body| normalize_model_items(provider, body))
    {
        let id = model
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if seen.insert(id) {
            merged.push(model);
        }
    }
    merged
}

#[tauri::command]
async fn validate_model_provider(
    app: AppHandle,
    settings: AppSettings,
    api_key: Option<String>,
) -> Result<Value, String> {
    observe_security_settings(&settings);
    let provider = settings.provider.trim().to_lowercase();
    let base_url = provider_base_url(&provider, &settings.base_url);
    let supplied_key = api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if let Some(key) = supplied_key.as_deref() {
        save_api_key(app.clone(), key.to_owned(), Some(provider.clone()))?;
    }
    let key = if provider == "ollama" {
        "ollama".to_owned()
    } else {
        supplied_key.unwrap_or(load_api_key(&app, &provider)?)
    };
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(
            settings.llm_timeout_seconds.max(30),
        ))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let base_models_url = format!("{}/models", base_url.trim_end_matches('/'));
    let urls = if provider == "siliconflow" {
        vec![
            base_models_url.clone(),
            format!("{base_models_url}?type=text"),
            format!("{base_models_url}?type=image"),
            format!("{base_models_url}?type=audio"),
            format!("{base_models_url}?type=video"),
        ]
    } else {
        vec![base_models_url]
    };
    let mut bodies = Vec::new();
    let mut last_error: Option<String> = None;
    for url in urls {
        let mut request = client.get(&url);
        if provider != "ollama" {
            request = request.bearer_auth(&key);
        }
        if provider == "anthropic" {
            request = request.header("anthropic-version", "2023-06-01");
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("model list request failed: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("model list response parse failed: {e}"))?;
        if status.is_success() {
            bodies.push(body);
        } else {
            last_error = Some(format!("model service returned {status}: {body}"));
        }
    }
    let models = merge_model_items(&provider, &bodies);
    if models.is_empty() {
        return Err(last_error.unwrap_or_else(|| {
            "model service connected but returned no recognizable models".to_owned()
        }));
    }
    Ok(json!({
        "ok": true,
        "provider": provider,
        "baseUrl": base_url,
        "models": models,
        "enabledModels": settings.enabled_models,
        "knownModels": settings.available_models,
        "message": format!("验证成功，已获取 {} 个可用模型。", models.len())
    }))
}

fn file_api_key(app: &AppHandle, provider: &str) -> Option<String> {
    api_key_fallback_file(app, provider)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[tauri::command]
fn save_api_key(app: AppHandle, api_key: String, provider: Option<String>) -> Result<bool, String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("API Key 涓嶈兘涓虹┖".into());
    }
    let provider = provider.unwrap_or_else(|| "compatible".to_owned());

    set_cached_api_key(&provider, trimmed);
    let fallback = api_key_fallback_file(&app, &provider)?;
    fs::write(&fallback, trimmed).map_err(|e| {
        format!("API Key 宸茶繘鍏ヤ細璇濈紦瀛橈紝浣嗗啓鍏ユ湰鍦板厹搴曟枃浠跺け璐? {e}")
    })?;

    if let Ok(entry) = credential(&provider) {
        if let Err(error) = entry.set_password(trimmed) {
            tracing::warn!(
                "failed to save API Key to Windows credential store; local fallback is available: {error}"
            );
        }
    }
    Ok(true)
}

#[tauri::command]
fn has_api_key(app: AppHandle, provider: String) -> Result<bool, String> {
    if provider == "ollama" {
        return Ok(true);
    }
    if cached_api_key(&provider).is_some()
        || env_api_key(&provider).is_some()
        || file_api_key(&app, &provider).is_some()
    {
        return Ok(true);
    }
    Ok(credential(&provider)
        .and_then(|entry| entry.get_password().map_err(|e| e.to_string()))
        .map(|password| !password.trim().is_empty())
        .unwrap_or(false))
}

fn load_api_key(app: &AppHandle, provider: &str) -> Result<String, String> {
    if provider == "ollama" {
        return Ok("ollama".into());
    }
    if let Some(api_key) = cached_api_key(provider) {
        return Ok(api_key);
    }
    if let Some(api_key) = env_api_key(provider) {
        set_cached_api_key(provider, &api_key);
        return Ok(api_key);
    }
    if let Ok(entry) = credential(provider) {
        if let Ok(password) = entry.get_password() {
            let trimmed = password.trim();
            if !trimmed.is_empty() {
                set_cached_api_key(provider, trimmed);
                return Ok(trimmed.to_owned());
            }
        }
    }
    if let Some(api_key) = file_api_key(app, provider) {
        set_cached_api_key(provider, &api_key);
        return Ok(api_key);
    }
    Err("API Key is not configured. Please save it in Settings first.".to_owned())
}

fn default_workspace_dir() -> Result<PathBuf, String> {
    let dir = directories::UserDirs::new()
        .and_then(|dirs| dirs.document_dir().map(PathBuf::from))
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Documents")))
        .unwrap_or_else(|| PathBuf::from("C:/XiaohengWorkBuddy"))
        .join("XiaohengWorkBuddy");
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create default workspace: {e}"))?;
    Ok(dir)
}

fn active_workspace_dir(work_context: &Value) -> Result<PathBuf, String> {
    let selected = work_context
        .get("activeConversation")
        .and_then(|conversation| conversation.get("options"))
        .and_then(|options| options.get("workspaceRoot"))
        .and_then(Value::as_str)
        .or_else(|| {
            work_context
                .get("settings")
                .and_then(|settings| settings.get("workspaceRoot"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .trim();
    let dir = if selected.is_empty() {
        default_workspace_dir()?
    } else {
        PathBuf::from(selected)
    };
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create workspace: {e}"))?;
    Ok(dir)
}

fn setting_bool(work_context: &Value, key: &str, default: bool) -> bool {
    let mode = session_permission_mode(work_context);
    match (mode.as_str(), key) {
        ("full", "allowOutsideWorkspace") => return true,
        ("full", "confirmShell") | ("full", "confirmFileWrite") => return false,
        ("restricted", "allowOutsideWorkspace") | ("approval", "allowOutsideWorkspace") => {
            return false
        }
        ("restricted", "confirmShell")
        | ("restricted", "confirmFileWrite")
        | ("approval", "confirmShell")
        | ("approval", "confirmFileWrite") => return true,
        _ => {}
    }
    work_context
        .get("settings")
        .and_then(|settings| settings.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn session_permission_mode(work_context: &Value) -> String {
    work_context
        .get("settings")
        .and_then(|settings| settings.get("sessionPermissionMode"))
        .and_then(Value::as_str)
        .or_else(|| {
            work_context
                .get("activeConversation")
                .and_then(|conversation| conversation.get("options"))
                .and_then(|options| options.get("permissionMode"))
                .and_then(Value::as_str)
        })
        .filter(|mode| ["restricted", "approval", "full"].contains(mode))
        .unwrap_or("approval")
        .to_owned()
}

fn permission_required_message(action: &str) -> String {
    format!(
        "APPROVAL_REQUIRED: 当前会话权限不足，已阻止 {action}。请在当前对话底部的“权限”下拉框中切换到“完全访问”，然后重新发送或继续任务。仅本会话会获得 Shell、文件写入和工作区外访问权限。"
    )
}

fn desktop_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(PathBuf::from))
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Desktop")))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join("Desktop")))
        .unwrap_or_else(env::temp_dir)
}

fn latest_user_prompt_from_messages(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_owned())
        .unwrap_or_default()
}

fn is_continue_prompt(prompt: &str) -> bool {
    matches!(
        compact_prompt_text(prompt).as_str(),
        "继续" | "继续执行" | "继续吧" | "接着来" | "接着做" | "继续上一步" | "go on" | "continue"
    )
}

fn is_meta_error_analysis_prompt(prompt: &str) -> bool {
    let compact = compact_prompt_text(prompt);
    (compact.contains("报错") || compact.contains("错误") || compact.contains("error"))
        && (compact.contains("分析")
            || compact.contains("原因")
            || compact.contains("什么原因")
            || compact.contains("为什么")
            || compact.contains("怎么回事"))
}

fn is_actionable_resume_source(prompt: &str) -> bool {
    !is_continue_prompt(prompt) && !is_meta_error_analysis_prompt(prompt)
}

fn effective_user_prompt_from_messages(messages: &[ChatMessage]) -> String {
    let latest = latest_user_prompt_from_messages(messages);
    if !is_continue_prompt(&latest) {
        return latest;
    }
    messages
        .iter()
        .rev()
        .filter(|message| message.role == "user")
        .skip(1)
        .find(|message| is_actionable_resume_source(&message.content))
        .map(|message| format!("继续执行上一条真实任务：\n{}", message.content.trim()))
        .unwrap_or(latest)
}

fn compact_prompt_text(prompt: &str) -> String {
    prompt
        .trim()
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '。' | '！' | '!' | '?' | '？' | '~' | '～' | '.' | ',' | '，'
                )
        })
        .to_lowercase()
}

fn simple_gateway_reply(prompt: &str) -> Option<String> {
    let compact = compact_prompt_text(prompt);
    let reply = match compact.as_str() {
        "你好" | "您好" | "嗨" | "哈喽" | "hello" | "hi" | "hey" | "在吗" | "在不在" => {
            "在，我是小亨。你可以直接把要做的事发给我；如果只是聊天，我会直接回复，如果需要文件、Shell 或桌面操作，我会按会话权限执行。"
        }
        "谢谢" | "感谢" | "多谢" | "thanks" | "thank you" => "不客气。",
        "好的" | "好" | "ok" | "收到" | "明白" => "收到。",
        _ => return None,
    };
    Some(reply.to_owned())
}

fn asks_artifact_generation(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    lower.contains("ppt")
        || lower.contains("pptx")
        || lower.contains("ppf")
        || lower.contains("powerpoint")
        || lower.contains("excel")
        || lower.contains("xlsx")
        || lower.contains("word")
        || lower.contains("docx")
        || lower.contains("pdf")
        || lower.contains(".md")
        || lower.contains("markdown")
        || lower.contains("image")
        || lower.contains("video")
        || prompt.contains("图片")
        || prompt.contains("图像")
        || prompt.contains("视频")
        || prompt.contains("文档")
        || prompt.contains("表格")
        || prompt.contains("文件")
        || prompt.contains("保存")
        || prompt.contains("导出")
        || prompt.contains("放在")
        || prompt.contains("桌面")
}

fn prompt_requires_real_tool_execution(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    asks_artifact_generation(prompt)
        || prompt_matches_jimeng_image(prompt)
        || (prompt_matches_dispatch_workflow(prompt)
            && (prompt.contains("生成")
                || prompt.contains("制作")
                || prompt.contains("创建")
                || prompt.contains("执行")
                || prompt.contains("分别")
                || lower.contains("generate")
                || lower.contains("create")))
        || lower.contains("shell")
        || lower.contains("powershell")
        || lower.contains("cmd")
        || lower.contains("npm")
        || lower.contains("cargo")
        || lower.contains("build")
        || lower.contains("compile")
        || lower.contains("download")
        || prompt.contains("执行")
        || prompt.contains("运行")
        || prompt.contains("安装")
        || prompt.contains("编译")
        || prompt.contains("打包")
        || prompt.contains("写入")
        || prompt.contains("下载")
}

fn asks_installed_skill_list(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    if asks_artifact_generation(prompt) {
        return false;
    }
    (prompt.contains("技能") || lower.contains("skill"))
        && (prompt.contains("列出")
            || prompt.contains("所有")
            || prompt.contains("全部")
            || prompt.contains("已安装")
            || prompt.contains("已启用")
            || lower.contains("list")
            || lower.contains("installed")
            || lower.contains("enabled"))
}

fn context_generated_at(work_context: &Value) -> Option<String> {
    work_context
        .get("generatedAt")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn context_today(work_context: &Value) -> String {
    context_generated_at(work_context)
        .and_then(|value| value.get(0..10).map(str::to_owned))
        .unwrap_or_else(|| "2026-06-29".to_owned())
}

fn add_hours_to_utc_iso(iso: &str, hours_to_add: i32) -> Option<String> {
    let year: i32 = iso.get(0..4)?.parse().ok()?;
    let month: i32 = iso.get(5..7)?.parse().ok()?;
    let day: i32 = iso.get(8..10)?.parse().ok()?;
    let hour: i32 = iso.get(11..13)?.parse().ok()?;
    let minute = iso.get(14..16)?;
    let second = iso.get(17..19)?;
    let days_in_month = |y: i32, m: i32| -> i32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
            2 => 28,
            _ => 30,
        }
    };
    let mut y = year;
    let mut m = month;
    let mut d = day;
    let mut h = hour + hours_to_add;
    while h >= 24 {
        h -= 24;
        d += 1;
        if d > days_in_month(y, m) {
            d = 1;
            m += 1;
            if m > 12 {
                m = 1;
                y += 1;
            }
        }
    }
    while h < 0 {
        h += 24;
        d -= 1;
        if d < 1 {
            m -= 1;
            if m < 1 {
                m = 12;
                y -= 1;
            }
            d = days_in_month(y, m);
        }
    }
    Some(format!("{y:04}-{m:02}-{d:02} {h:02}:{minute}:{second}"))
}

fn current_time_reply(work_context: &Value, prompt: &str) -> Option<String> {
    let lower = prompt.to_lowercase();
    let asks_time = prompt.contains("实际时间")
        || prompt.contains("现在的时间")
        || prompt.contains("当前时间")
        || prompt.contains("今天几号")
        || prompt.contains("现在几点")
        || (prompt.contains("现在") && (prompt.contains("时间") || prompt.contains("日期")))
        || lower.contains("current time")
        || lower.contains("actual time");
    if !asks_time {
        return None;
    }
    let generated_at = context_generated_at(work_context).unwrap_or_else(|| now_string());
    let beijing = add_hours_to_utc_iso(&generated_at, 8).unwrap_or_else(|| generated_at.clone());
    Some(format!(
        "当前时间以 WorkBuddy 前端生成的会话时间为准：\n\n- UTC/ISO：`{generated_at}`\n- 北京时间：`{beijing}`\n\n如果需要核对 Windows 系统时钟，应执行 PowerShell `Get-Date`，不要让模型用 Python 代码或训练记忆猜时间。"
    ))
}

fn installed_skill_list_reply(work_context: &Value, prompt: &str) -> Option<String> {
    if !asks_installed_skill_list(prompt) {
        return None;
    }
    let mut skills = work_context
        .get("installedSkills")
        .or_else(|| work_context.get("enabledSkills"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if work_context.get("installedSkills").is_none() {
        let disabled = work_context
            .get("disabledSkills")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        skills.extend(disabled);
    }
    if skills.is_empty() {
        return Some("当前会话上下文里没有已安装技能。".to_owned());
    }
    skills.sort_by_key(|skill| {
        skill
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_lowercase()
    });
    let rows = skills
        .iter()
        .enumerate()
        .map(|(index, skill)| {
            let name = skill
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("未命名技能");
            let enabled = skill
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let source = skill
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let category = skill
                .get("category")
                .and_then(Value::as_str)
                .unwrap_or(source);
            let description = skill
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(80)
                .collect::<String>()
                .replace('\n', " ");
            format!(
                "| {} | {} | {} | {} | {} |",
                index + 1,
                name,
                if enabled { "启用" } else { "停用" },
                category,
                description
            )
        })
        .collect::<Vec<_>>();
    Some(format!(
        "当前已安装技能共 {} 个：\n\n| # | 技能 | 状态 | 来源/分类 | 说明 |\n|---:|---|---|---|---|\n{}\n\n这是从当前会话技能状态直接读取的结果，没有执行 Shell，也没有创建或修改工作空间。",
        rows.len(),
        rows.join("\n")
    ))
}

fn jimeng_image_skill_summary() -> Value {
    json!({
        "id": "jimeng-image",
        "name": "jimeng-image",
        "category": "image",
        "enabled": true,
        "source": "builtin-alias",
        "description": "图片生成技能。优先使用 Seedream 5.0 Lite OpenAI 兼容接口生成图片并下载到本地；需要4K或图生图时再使用即梦4.0。",
        "tools": ["skill_read", "write_text_file", "shell_exec", "list_files", "read_text_file"]
    })
}

fn jimeng_image_skill_content() -> &'static str {
    r#"jimeng-image 技能说明：
- 用途：文生图、图生图、多图组合生成。用户提到 jimeng、即梦、Seedream、生图、生成图片时优先使用本技能，不要先打开网页。
- 默认路径：优先使用 Seedream 5.0 Lite（火山引擎 Agent Plan OpenAI 兼容接口），除非用户要求4K超高清或图生图。
- API：POST https://ark.cn-beijing.volces.com/api/plan/v3/images/generations
- 模型：doubao-seedream-5-0-lite
- 鉴权：Authorization: Bearer ark-4746d3da-36e6-46cb-b604-6e617d1c549c-3874f
- 推荐参数：n=1，size=2048x2048，timeout=60。
- 正确执行链路：
  1. 写一个最小 Python 脚本到工作区或桌面；
  2. 脚本用 requests 调用 images/generations；
  3. 读取 data[0].url；
  4. 下载图片到本地文件，例如 jimeng_cat_butterfly.png；
  5. 用 list_files 或 shell_exec 验证文件存在、大小合理、格式可打开；
  6. 最终回复只报告真实生成的本地路径。
- 不要只口头声称已生成；没有工具返回的本地文件路径和验证结果时，不能说任务完成。
- approval 权限下如需 shell 或写文件，应请求批准；full 权限下直接执行。"#
}

fn prompt_matches_jimeng_image(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    lower.contains("jimeng")
        || lower.contains("seedream")
        || prompt.contains("即梦")
        || prompt.contains("生图")
        || prompt.contains("文生图")
        || (prompt.contains("图片") && (prompt.contains("生成") || prompt.contains("画")))
        || (prompt.contains("图像") && prompt.contains("生成"))
}

fn prompt_matches_dispatch_workflow(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let mentions_people = prompt.contains("人员")
        || prompt.contains("成员")
        || prompt.contains("小组")
        || prompt.contains("经理")
        || prompt.contains("产品经理")
        || prompt.contains("内容经理")
        || prompt.contains("负责人")
        || lower.contains("agent")
        || lower.contains("team")
        || lower.contains("manager");
    let mentions_dispatch = prompt.contains("调度")
        || prompt.contains("派发")
        || prompt.contains("分配")
        || prompt.contains("分别")
        || prompt.contains("协作")
        || prompt.contains("拆分")
        || prompt.contains("拆解")
        || prompt.contains("任务空间")
        || prompt.contains("任务中心")
        || prompt.contains("工作状态")
        || prompt.contains("人员卡片")
        || lower.contains("todo")
        || lower.contains("dispatch")
        || lower.contains("delegate")
        || lower.contains("assign")
        || lower.contains("workflow");
    let asks_people_to_work = mentions_people
        && (prompt.contains("生成")
            || prompt.contains("制作")
            || prompt.contains("创建")
            || prompt.contains("执行")
            || prompt.contains("完成")
            || prompt.contains("负责")
            || lower.contains("create")
            || lower.contains("generate")
            || lower.contains("run"));
    mentions_dispatch || asks_people_to_work
}

fn triggered_skill_hints(prompt: &str) -> String {
    if prompt_matches_jimeng_image(prompt) {
        format!(
            "命中技能：jimeng-image。必须先调用 skill_read(jimeng-image) 或按该技能说明走 Seedream 5.0 Lite API 生成并下载图片；不要改走浏览器网页自动化，除非 API 明确失败。"
        )
    } else {
        String::new()
    }
}

fn triggered_dispatch_hints(prompt: &str) -> String {
    if prompt_matches_dispatch_workflow(prompt) {
        "命中调度任务：用户要求派发/拆分/多人协作/人员状态同步时，必须先调用 team_roster 确认可用人员；再用 task_plan_update 或 work_pool_update 创建/更新 todo、负责人、状态和交付物；执行中每完成一个阶段都要同步状态，避免只在聊天里口头说明。".to_string()
    } else {
        String::new()
    }
}

fn concise_gateway_execution_system(work_context: &Value, latest_prompt: &str) -> String {
    let workspace_root = work_context
        .get("settings")
        .and_then(|settings| settings.get("workspaceRoot"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let permission_mode = session_permission_mode(work_context);
    let enabled_skills = work_context
        .get("enabledSkills")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|skill| {
                    let name = skill.get("name").and_then(Value::as_str)?;
                    let description = skill
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    Some(format!("{name}: {description}"))
                })
                .take(12)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let active_tasks = work_context
        .get("tasks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|task| {
                    let title = task.get("title").and_then(Value::as_str)?;
                    let status = task.get("status").and_then(Value::as_str).unwrap_or("todo");
                    Some(format!("{status}:{title}"))
                })
                .take(8)
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();
    let desktop = desktop_dir().to_string_lossy().to_string();
    let generated_at = context_generated_at(work_context).unwrap_or_else(|| now_string());
    let beijing_time =
        add_hours_to_utc_iso(&generated_at, 8).unwrap_or_else(|| generated_at.clone());
    let artifact_rule = artifact_instruction(latest_prompt);
    let skill_hints = triggered_skill_hints(latest_prompt);
    let dispatch_hints = triggered_dispatch_hints(latest_prompt);
    format!(
        "你是小亨 WorkBuddy Gateway 的模型脑。Gateway 负责执行工具，你负责理解需求、选择技能/工具、写代码、组织内容和判断下一步。\n\
当前请求：{latest_prompt}\n\
环境摘要：workspaceRoot={workspace_root}；desktop={desktop}；permission={permission_mode}。\n\
当前时间：UTC/ISO={generated_at}；北京时间={beijing_time}。涉及“今天/现在/最近”必须以这里或真实工具返回为准，不得凭模型记忆或伪代码猜时间。\n\
当前任务摘要：{active_tasks}\n\
可用技能摘要：\n{enabled_skills}\n\
技能触发提示：{skill_hints}\n\
调度触发提示：{dispatch_hints}\n\
执行规则：{artifact_rule}\n\
工作方式：\n\
1. 如果需要工具，先在 assistant content 中用 1 句话确认收到任务，并列出 3-5 个简短步骤，然后同时发出最少必要工具调用。\n\
2. 文件制品任务应像 OpenClaw 一样执行：确认目标路径/环境 -> 一次性写完整脚本或文件 -> 执行 -> 验证输出 -> 最终汇报。\n\
3. 不要把大纲、分块阅读或询问是否继续当作交付；能一次性生成就一次性生成。\n\
4. 简单聊天不调用工具；只有真实文件、Shell、网页、技能读取、项目状态变更才调用工具。\n\
5. 普通问答不要创建任务中心 todo；但用户明确要求调度、分配、分别执行、多人协作或任务空间可视化时，必须同步任务中心和人员工作状态。\n\
6. approval/restricted 权限下不要执行 Shell 或写文件；需要时让 Gateway 请求用户批准。\n\
7. 用户只是查询/列出/检查技能、记忆、设置、任务或状态时，只读回答或调用只读工具；不得创建 Workspace、目录、任务、项目或初始化任何文件结构，除非用户明确要求初始化/创建。\n\
8. 用简体中文，过程简短，最终只报告真实结果、文件路径和验证信息。"
    )
}

fn maybe_emit_simple_gateway_response(
    app: &AppHandle,
    run_id: &str,
    messages: &[ChatMessage],
    work_context: &Value,
) -> bool {
    let prompt = latest_user_prompt_from_messages(messages);
    let Some(reply) = simple_gateway_reply(&prompt)
        .or_else(|| current_time_reply(work_context, &prompt))
        .or_else(|| installed_skill_list_reply(work_context, &prompt))
    else {
        return false;
    };
    emit_stream(
        app,
        run_id,
        "started",
        json!({"message": "小亨 Gateway 已直接处理这条轻量消息。"}),
    );
    emit_stream(
        app,
        run_id,
        "done",
        json!({
            "content": reply,
            "tasks": [],
            "workPoolUpdates": {},
            "todos": [],
            "toolSteps": []
        }),
    );
    true
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn path_starts_with(candidate: &Path, base: &Path) -> bool {
    if cfg!(windows) {
        let candidate = candidate
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase();
        let mut base = base.to_string_lossy().replace('/', "\\").to_lowercase();
        if !base.ends_with('\\') {
            base.push('\\');
        }
        candidate == base.trim_end_matches('\\') || candidate.starts_with(&base)
    } else {
        candidate.starts_with(base)
    }
}

fn resolve_tool_path(path: PathBuf, work_context: &Value) -> Result<PathBuf, String> {
    let workspace = normalize_path(active_workspace_dir(work_context)?);
    let candidate = if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    };
    let candidate = normalize_path(candidate);
    if setting_bool(work_context, "allowOutsideWorkspace", false)
        || path_starts_with(&candidate, &workspace)
    {
        Ok(candidate)
    } else {
        Err(format!(
            "{} Workspace: {}",
            permission_required_message("访问工作区以外路径"),
            workspace.to_string_lossy()
        ))
    }
}

fn marketplace_root() -> PathBuf {
    env::var_os("XIAOHENG_MARKETPLACE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(OPENPISCI_MARKETPLACE_ROOT))
}

fn read_json_file(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn seed_marketplace() -> MarketplaceIndex {
    MarketplaceIndex {
        experts: vec![
            MarketItem {
                id: "openpisci/expert/research-analyst@1.0.0".into(),
                name: "Research Analyst".into(),
                description: "Research, compare, summarize and prepare sourced reports.".into(),
                download_url: None,
                source: Some("openpisci".into()),
                trusted: Some(true),
                tags: Some(vec!["research".into()]),
                featured: Some(true),
            },
            MarketItem {
                id: "openpisci/expert/document-skills@1.0.0".into(),
                name: "Document Specialist".into(),
                description: "Create and organize Word, Excel, PPT and PDF office documents."
                    .into(),
                download_url: None,
                source: Some("openpisci".into()),
                trusted: Some(true),
                tags: Some(vec!["office".into()]),
                featured: Some(true),
            },
            MarketItem {
                id: "openpisci/expert/skill-creator@1.0.0".into(),
                name: "Skill Creator".into(),
                description: "Define, write, test and install reusable Agent skills.".into(),
                download_url: None,
                source: Some("skillhub".into()),
                trusted: Some(true),
                tags: Some(vec!["skills".into()]),
                featured: Some(false),
            },
        ],
        teams: vec![
            MarketItem {
                id: "openpisci/team/content-squad@1.0.0".into(),
                name: "Content Squad".into(),
                description: "Plan, write, review and publish content as a team.".into(),
                download_url: None,
                source: Some("openpisci".into()),
                trusted: Some(true),
                tags: Some(vec!["content".into()]),
                featured: Some(true),
            },
            MarketItem {
                id: "openpisci/team/data-analysis@1.0.0".into(),
                name: "Data Analysis Team".into(),
                description: "Research, clean, analyze, visualize and report data.".into(),
                download_url: None,
                source: Some("clawhub".into()),
                trusted: Some(true),
                tags: Some(vec!["data".into()]),
                featured: Some(true),
            },
        ],
        skills: vec![
            MarketItem {
                id: "openpisci/skill/xlsx@1.0.0".into(),
                name: "xlsx".into(),
                description: "Create, edit and validate Excel workbooks.".into(),
                download_url: None,
                source: Some("openai".into()),
                trusted: Some(true),
                tags: Some(vec!["office".into()]),
                featured: Some(true),
            },
            MarketItem {
                id: "openpisci/skill/docx@1.0.0".into(),
                name: "docx".into(),
                description: "Create, edit and review Word documents.".into(),
                download_url: None,
                source: Some("openai".into()),
                trusted: Some(true),
                tags: Some(vec!["office".into()]),
                featured: Some(true),
            },
            MarketItem {
                id: "openpisci/skill/frontend-design@1.0.0".into(),
                name: "frontend-design".into(),
                description: "Build high-quality frontend UI and components.".into(),
                download_url: None,
                source: Some("skillhub".into()),
                trusted: Some(true),
                tags: Some(vec!["design".into(), "frontend".into()]),
                featured: Some(false),
            },
        ],
    }
}

fn local_marketplace_index() -> MarketplaceIndex {
    let root = marketplace_root();
    let index_path = root.join("index.json");
    let mut index: MarketplaceIndex = read_json_file(&index_path)
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(seed_marketplace);

    let seed = seed_marketplace();
    if index.experts.is_empty() {
        index.experts = seed.experts;
    }
    if index.teams.is_empty() {
        index.teams = seed.teams;
    }
    if index.skills.is_empty() {
        index.skills = seed.skills;
    }
    index
}

#[tauri::command]
fn fetch_marketplace_index() -> Result<Value, String> {
    let index = local_marketplace_index();
    serde_json::to_value(index).map_err(|e| e.to_string())
}

#[tauri::command]
fn fetch_marketplace_aggregated() -> Result<Value, String> {
    let index = local_marketplace_index();
    Ok(json!({
        "source": marketplace_root().to_string_lossy(),
        "experts": index.experts,
        "teams": index.teams,
        "skills": index.skills,
        "skillMarkets": ["OpenPiscis", "clawhub", "skillhub"]
    }))
}

fn registry_base(source: &str) -> Result<&'static str, String> {
    match source {
        "harmony" => Ok(HARMONY_MARKET_URL),
        "clawhub" => Ok("https://clawhub.ai"),
        "skillhub" => Ok("https://api.skillhub.cn"),
        other => Err(format!("unsupported skill registry: {other}")),
    }
}

fn registry_result_to_market_item(source: &str, value: &Value) -> Option<Value> {
    let slug = value["slug"].as_str()?.to_owned();
    let name = value["displayName"]
        .as_str()
        .or_else(|| value["name"].as_str())
        .unwrap_or(&slug)
        .to_owned();
    let description = value["summary"]
        .as_str()
        .or_else(|| value["description_zh"].as_str())
        .or_else(|| value["description"].as_str())
        .unwrap_or("")
        .to_owned();
    let version = value["version"]
        .as_str()
        .or_else(|| value["latestVersion"]["version"].as_str())
        .or_else(|| value["tags"]["latest"].as_str())
        .unwrap_or("latest")
        .to_owned();
    let downloads = value["downloads"]
        .as_u64()
        .or_else(|| value["installs"].as_u64())
        .or_else(|| value["stats"]["installsAllTime"].as_u64())
        .or_else(|| value["stats"]["downloads"].as_u64())
        .unwrap_or(0);
    let stars = value["stars"]
        .as_u64()
        .or_else(|| value["stats"]["stars"].as_u64())
        .unwrap_or(0);
    let tags: Vec<String> = value["topics"]
        .as_array()
        .or_else(|| value["tags"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(json!({
        "id": format!("{source}/skill/{slug}@{version}"),
        "name": name,
        "description": description,
        "source": source,
        "trusted": true,
        "tags": tags,
        "featured": stars > 100,
        "slug": slug,
        "version": version,
        "downloads": downloads,
        "stars": stars,
        "download_url": format!("{}/api/v1/skills/{}", registry_base(source).ok()?, slug)
    }))
}

async fn search_skill_market_core(
    client: &reqwest::Client,
    source: &str,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Value, String> {
    let base = registry_base(source)?;
    if source == "harmony" {
        let url = format!("{base}/api/v1/skills");
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("failed to connect harmony: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("failed to parse harmony response: {e}"))?;
        if !status.is_success() {
            return Err(format!("harmony returned {status}: {body}"));
        }
        let items: Vec<Value> = body["skills"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(harmony_result_to_market_item)
            .filter(|item| {
                if query.trim().is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {}",
                    item["name"].as_str().unwrap_or_default(),
                    item["description"].as_str().unwrap_or_default(),
                    item["tags"]
                        .as_array()
                        .map(|tags| format!("{tags:?}"))
                        .unwrap_or_default()
                )
                .to_lowercase();
                haystack.contains(&query.trim().to_lowercase())
            })
            .collect();
        let total = items.len();
        return Ok(json!({
            "source": source,
            "query": query,
            "items": items,
            "total": total
        }));
    }

    let url = if source == "clawhub" && query.trim().is_empty() {
        format!("{base}/api/v1/skills?limit={limit}&offset={offset}")
    } else {
        format!(
            "{base}/api/v1/search?q={}&limit={limit}&offset={offset}",
            percent_encode(query.trim())
        )
    };
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("failed to connect {source}: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("failed to parse {source} response: {e}"))?;
    if !status.is_success() {
        return Err(format!("{source} returned {status}: {body}"));
    }

    let raw_items = body["results"]
        .as_array()
        .or_else(|| body["items"].as_array())
        .cloned()
        .unwrap_or_default();
    let items: Vec<Value> = raw_items
        .iter()
        .filter_map(|item| registry_result_to_market_item(source, item))
        .collect();
    Ok(json!({
        "source": source,
        "query": query,
        "items": items,
        "total": items.len(),
        "nextCursor": body.get("nextCursor").cloned().unwrap_or(Value::Null)
    }))
}

#[tauri::command]
async fn search_skill_market(request: SkillMarketSearchRequest) -> Result<Value, String> {
    let source = request.source.trim().to_lowercase();
    let query = request.query.unwrap_or_default();
    let limit = request.limit.unwrap_or(24).min(50);
    let offset = request.offset.unwrap_or(0);
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    search_skill_market_core(&client, &source, &query, limit, offset).await
}

fn harmony_result_to_market_item(value: &Value) -> Option<Value> {
    let id = value["id"].as_str()?.to_owned();
    let name = value["name"].as_str().unwrap_or(&id).to_owned();
    let description = value["description"].as_str().unwrap_or("").to_owned();
    let version = value["version"].as_str().unwrap_or("latest").to_owned();
    let tags = value["tags"].as_array().cloned().unwrap_or_default();
    let raw_download_url = value["download_url"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{HARMONY_MARKET_URL}/api/v1/skills/{}/download",
                percent_encode(&id)
            )
        });
    let download_url = if raw_download_url.starts_with('/') {
        format!("{HARMONY_MARKET_URL}{raw_download_url}")
    } else {
        raw_download_url
    };
    Some(json!({
        "id": format!("harmony/skill/{id}@{version}"),
        "name": name,
        "description": description,
        "source": "harmony",
        "trusted": true,
        "tags": tags,
        "featured": value["featured"].as_bool().unwrap_or(false),
        "slug": id,
        "version": version,
        "download_url": download_url,
        "installPath": value["filename"].as_str().unwrap_or_default()
    }))
}

fn installed_skill_item(
    source: &str,
    slug: &str,
    name: &str,
    description: &str,
    version: &str,
    path: &Path,
) -> Value {
    json!({
        "id": format!("skill-{source}-{}", safe_slug(slug)),
        "name": name,
        "description": description,
        "enabled": true,
        "category": source,
        "source": source,
        "sourceId": format!("{source}:{slug}"),
        "installPath": path.to_string_lossy(),
        "version": version,
        "tools": ["skill_search", "skill_read"]
    })
}

fn collect_skill_md_files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_skill_md_files(&path, output)?;
        } else if path
            .file_name()
            .map(|name| name.to_string_lossy().eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false)
        {
            output.push(path);
        }
    }
    Ok(())
}

fn nearest_manifest(skill_md: &Path, root: &Path) -> Value {
    let mut current = skill_md.parent();
    while let Some(dir) = current {
        if let Some(manifest) = read_json_file(&dir.join("manifest.json")) {
            return manifest;
        }
        if let Some(manifest) = read_json_file(&dir.join("harmony.skill.json")) {
            return manifest;
        }
        if dir == root {
            break;
        }
        current = dir.parent();
    }
    json!({})
}

fn skill_source_slug_from_path(
    skill_md: &Path,
    root: &Path,
    manifest: &Value,
) -> (String, String, PathBuf) {
    let relative = skill_md.strip_prefix(root).unwrap_or(skill_md);
    let parts = relative
        .components()
        .filter_map(|part| match part {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let manifest_source = manifest["source"].as_str().map(str::to_owned);
    let manifest_slug = manifest["slug"].as_str().map(str::to_owned);
    if parts.first().map(String::as_str) == Some("skills") {
        let slug = manifest_slug
            .or_else(|| parts.get(1).cloned())
            .unwrap_or_else(|| "skill".to_owned());
        let source = manifest_source.unwrap_or_else(|| "local".to_owned());
        let dir = root
            .join("skills")
            .join(parts.get(1).cloned().unwrap_or_else(|| safe_slug(&slug)));
        return (source, slug, dir);
    }
    if parts.len() >= 2 {
        let source = manifest_source.unwrap_or_else(|| parts[0].clone());
        let slug = manifest_slug.unwrap_or_else(|| parts[1].clone());
        return (source, slug, root.join(&parts[0]).join(&parts[1]));
    }
    let slug = manifest_slug
        .or_else(|| {
            skill_md
                .parent()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "skill".to_owned());
    let source = manifest_source.unwrap_or_else(|| "local".to_owned());
    let dir = skill_md.parent().unwrap_or(root).to_path_buf();
    (source, slug, dir)
}

fn parse_skill_frontmatter(content: &str, fallback_name: &str) -> (String, String) {
    let mut name = fallback_name.to_owned();
    let mut description = String::new();
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            for line in rest[..end].lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("name:") {
                    name = value.trim().trim_matches('"').trim_matches('\'').to_owned();
                }
                if let Some(value) = trimmed.strip_prefix("description:") {
                    description = value.trim().trim_matches('"').trim_matches('\'').to_owned();
                }
            }
        }
    }
    if description.is_empty() {
        description = content
            .lines()
            .find(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("---")
            })
            .unwrap_or("")
            .chars()
            .take(240)
            .collect();
    }
    (name, description)
}

#[tauri::command]
async fn install_skill_from_locator(
    app: AppHandle,
    request: SkillLocatorInstallRequest,
) -> Result<Value, String> {
    let locator = request.locator.trim();
    if locator.is_empty() {
        return Err("璇疯緭鍏ユ湰鍦?SKILL.md 璺緞鎴?URL".into());
    }
    let mut local_source_dir: Option<PathBuf> = None;
    let content = if locator.starts_with("http://") || locator.starts_with("https://") {
        apply_configured_proxy(reqwest::Client::builder())
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Xiaoheng-WorkBuddy/0.1")
            .build()
            .map_err(|e| e.to_string())?
            .get(locator)
            .send()
            .await
            .map_err(|e| format!("涓嬭浇鎶€鑳藉け璐? {e}"))?
            .text()
            .await
            .map_err(|e| format!("璇诲彇鎶€鑳藉唴瀹瑰け璐? {e}"))?
    } else {
        let path = PathBuf::from(locator.trim_matches('"'));
        let skill_path = if path.is_dir() {
            local_source_dir = Some(path.clone());
            path.join("SKILL.md")
        } else {
            path
        };
        fs::read_to_string(&skill_path).map_err(|e| {
            format!("璇诲彇鏈湴鎶€鑳藉け璐ワ紝璇锋彁渚?SKILL.md 鏂囦欢鎴栨妧鑳界洰褰? {e}")
        })?
    };
    if !content.contains("description:") && !content.contains('#') {
        return Err("鎶€鑳藉唴瀹逛笉鍍忔湁鏁堢殑 SKILL.md".into());
    }
    let fallback = request
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("local-skill");
    let (name, description) = parse_skill_frontmatter(&content, fallback);
    let slug = safe_slug(&name);
    let dir = unified_skills_dir(&app)?.join(&slug);
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|e| format!("failed to replace existing skill dir: {e}"))?;
    }
    if let Some(source_dir) = local_source_dir {
        copy_skill_dir(&source_dir, &dir)?;
        fs::write(dir.join("SKILL.md"), content)
            .map_err(|e| format!("failed to write SKILL.md: {e}"))?;
    } else {
        fs::create_dir_all(&dir).map_err(|e| format!("failed to create skill dir: {e}"))?;
        fs::write(dir.join("SKILL.md"), content)
            .map_err(|e| format!("failed to write SKILL.md: {e}"))?;
    }
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&json!({
            "source": "local",
            "slug": slug,
            "name": name,
            "description": description,
            "version": "local",
            "locator": locator
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("failed to write manifest: {e}"))?;
    Ok(json!({
        "message": format!("宸插畨瑁呮湰鍦版妧鑳斤細{}", name),
        "skills": [installed_skill_item("local", &slug, &name, &description, "local", &dir)]
    }))
}

#[tauri::command]
async fn install_registry_skill(
    app: AppHandle,
    request: RegistrySkillInstallRequest,
) -> Result<Value, String> {
    let source = request.source.trim().to_lowercase();
    let base = registry_base(&source)?;
    let slug = safe_slug(&request.slug);
    if slug.is_empty() {
        return Err("skill slug cannot be empty".into());
    }

    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(if source == "harmony" {
            900
        } else {
            60
        }))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    if source == "harmony" {
        let detail_url = format!("{base}/api/v1/skills/{slug}");
        let detail: Value = client
            .get(&detail_url)
            .send()
            .await
            .map_err(|e| format!("failed to fetch harmony skill detail: {e}"))?
            .json()
            .await
            .map_err(|e| format!("failed to parse harmony skill detail: {e}"))?;
        let raw_download_url = detail["download_url"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{base}/api/v1/skills/{slug}/download"));
        let download_url = if raw_download_url.starts_with('/') {
            format!("{base}{raw_download_url}")
        } else {
            raw_download_url
        };
        let mut response = client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| format!("failed to download harmony skill: {e}"))?;
        let total = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("failed to read harmony skill package: {e}"))?
        {
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);
            let percent = if total > 0 {
                ((downloaded as f64 / total as f64) * 85.0).round() as u64
            } else {
                0
            };
            let _ = app.emit(
                "xiaoheng://skill-install-progress",
                json!({
                    "source": "harmony",
                    "slug": slug,
                    "downloaded": downloaded,
                    "total": total,
                    "percent": percent.min(85),
                    "stage": "downloading"
                }),
            );
        }
        let _ = app.emit(
            "xiaoheng://skill-install-progress",
            json!({"source":"harmony","slug":slug,"downloaded":downloaded,"total":total,"percent":85,"stage":"downloaded"}),
        );
        let dir = unified_skills_dir(&app)?.join(&slug);
        if dir.exists() {
            fs::remove_dir_all(&dir)
                .map_err(|e| format!("failed to replace existing skill: {e}"))?;
        }
        let _ = app.emit(
            "xiaoheng://skill-install-progress",
            json!({"source":"harmony","slug":slug,"downloaded":downloaded,"total":total,"percent":90,"stage":"unpacking"}),
        );
        unpack_zip_bytes(&bytes, &dir)?;
        let _ = app.emit(
            "xiaoheng://skill-install-progress",
            json!({"source":"harmony","slug":slug,"downloaded":downloaded,"total":total,"percent":96,"stage":"indexing"}),
        );
        let manifest = fs::read_to_string(dir.join("harmony.skill.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({}));
        let skill_md = find_skill_md(&dir)
            .and_then(|path| fs::read_to_string(path).ok())
            .unwrap_or_default();
        let fallback_name = request.name.as_deref().unwrap_or(&slug);
        let (parsed_name, parsed_description) = if skill_md.is_empty() {
            (
                fallback_name.to_owned(),
                request.description.clone().unwrap_or_default(),
            )
        } else {
            parse_skill_frontmatter(&skill_md, fallback_name)
        };
        let name = request
            .name
            .or_else(|| manifest["name"].as_str().map(str::to_owned))
            .unwrap_or(parsed_name);
        let description = request
            .description
            .or_else(|| manifest["description"].as_str().map(str::to_owned))
            .unwrap_or(parsed_description);
        let version = request
            .version
            .or_else(|| manifest["version"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "latest".to_owned());
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&json!({
                "source": "harmony",
                "slug": slug,
                "name": name,
                "description": description,
                "version": version,
                "detailUrl": detail_url,
                "downloadUrl": download_url
            }))
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("failed to write manifest: {e}"))?;
        let _ = app.emit(
            "xiaoheng://skill-install-progress",
            json!({"source":"harmony","slug":slug,"downloaded":downloaded,"total":total,"percent":100,"stage":"done"}),
        );
        return Ok(json!({
            "message": format!("宸插畨瑁?Harmony 鎶€鑳斤細{}", name),
            "skills": [installed_skill_item("harmony", &slug, &name, &description, &version, &dir)]
        }));
    }

    let detail_url = format!("{base}/api/v1/skills/{slug}");
    let _ = app.emit(
        "xiaoheng://skill-install-progress",
        json!({"source":source,"slug":slug,"percent":10,"stage":"fetching"}),
    );
    let detail: Value = client
        .get(&detail_url)
        .send()
        .await
        .map_err(|e| format!("failed to fetch skill detail: {e}"))?
        .json()
        .await
        .map_err(|e| format!("failed to parse skill detail: {e}"))?;
    let _ = app.emit(
        "xiaoheng://skill-install-progress",
        json!({"source":source,"slug":slug,"percent":45,"stage":"downloaded"}),
    );

    let skill_value = detail.get("skill").unwrap_or(&detail);
    let name = request
        .name
        .or_else(|| skill_value["displayName"].as_str().map(str::to_owned))
        .or_else(|| skill_value["name"].as_str().map(str::to_owned))
        .unwrap_or_else(|| slug.clone());
    let description = request
        .description
        .or_else(|| skill_value["summary_zh"].as_str().map(str::to_owned))
        .or_else(|| skill_value["summary"].as_str().map(str::to_owned))
        .or_else(|| skill_value["description"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let version = request
        .version
        .or_else(|| {
            detail["latestVersion"]["version"]
                .as_str()
                .map(str::to_owned)
        })
        .or_else(|| skill_value["tags"]["latest"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "latest".to_owned());

    let skill_md = detail["skill"]["description"]
        .as_str()
        .or_else(|| detail["description"].as_str())
        .filter(|content| content.contains("SKILL.md") || content.contains("---"))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "---\nname: {}\ndescription: \"{}\"\n---\n\n# {}\n\n{}\n\nSource: {} / {}\n",
                name.replace('"', "'"),
                description.replace('"', "'"),
                name,
                description,
                source,
                slug
            )
        });

    let dir = unified_skills_dir(&app)?.join(&slug);
    let _ = app.emit(
        "xiaoheng://skill-install-progress",
        json!({"source":source,"slug":slug,"percent":75,"stage":"unpacking"}),
    );
    fs::create_dir_all(&dir).map_err(|e| format!("failed to create skill dir: {e}"))?;
    fs::write(dir.join("SKILL.md"), skill_md)
        .map_err(|e| format!("failed to write SKILL.md: {e}"))?;
    fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&json!({
            "source": source,
            "slug": slug,
            "name": name,
            "description": description,
            "version": version,
            "detailUrl": detail_url
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("failed to write manifest: {e}"))?;
    let _ = app.emit(
        "xiaoheng://skill-install-progress",
        json!({"source":source,"slug":slug,"percent":100,"stage":"done"}),
    );

    Ok(json!({
        "message": format!("宸插畨瑁呮妧鑳斤細{}", name),
        "skills": [installed_skill_item(&source, &slug, &name, &description, &version, &dir)]
    }))
}

#[tauri::command]
fn list_installed_market_skills(app: AppHandle) -> Result<Value, String> {
    let root = installed_skills_dir(&app)?;
    let mut skill_files = Vec::new();
    collect_skill_md_files(&root, &mut skill_files)?;
    let mut skills = Vec::new();
    let mut seen = BTreeSet::new();
    for skill_md in skill_files {
        let manifest = nearest_manifest(&skill_md, &root);
        let (source, slug, dir) = skill_source_slug_from_path(&skill_md, &root, &manifest);
        let key = format!("{}:{}", source, slug);
        if !seen.insert(key) {
            continue;
        }
        let content = fs::read_to_string(&skill_md).unwrap_or_default();
        let fallback_name = manifest["name"].as_str().unwrap_or(&slug);
        let (parsed_name, parsed_description) = parse_skill_frontmatter(&content, fallback_name);
        let name = manifest["name"].as_str().unwrap_or(&parsed_name).to_owned();
        let description = manifest["description"]
            .as_str()
            .unwrap_or(&parsed_description)
            .to_owned();
        let version = manifest["version"].as_str().unwrap_or("latest").to_owned();
        skills.push(installed_skill_item(
            &source,
            &slug,
            &name,
            &description,
            &version,
            &dir,
        ));
    }
    Ok(json!({ "skills": skills }))
}

#[tauri::command]
fn uninstall_market_skill(app: AppHandle, source: String, slug: String) -> Result<Value, String> {
    let source = safe_slug(&source.trim().to_lowercase());
    let slug = safe_slug(&slug);
    if source.is_empty() || slug.is_empty() {
        return Err("missing skill source or slug".to_owned());
    }
    let root = installed_skills_dir(&app)?;
    let candidates = [
        root.join("skills").join(&slug),
        root.join(&source).join(&slug),
    ];
    let mut removed = false;
    for dir in candidates {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("failed to uninstall skill: {e}"))?;
            removed = true;
        }
    }
    Ok(json!({ "ok": true, "source": source, "slug": slug, "removed": removed }))
}

#[tauri::command]
fn install_market_expert(request: InstallRequest) -> Result<Value, String> {
    let item = request.item;
    Ok(json!({
        "message": format!("宸插畨瑁呬笓瀹讹細{}", item.name),
        "agents": [{
            "id": format!("expert-{}", slug_id(&item.id)),
            "name": item.name,
            "role": "har",
            "title": "甯傚満涓撳",
            "specialty": item.description,
            "status": "online",
            "color": "#0f766e",
            "sourceId": item.id
        }]
    }))
}

#[tauri::command]
fn install_market_team(request: InstallRequest) -> Result<Value, String> {
    let item = request.item;
    let base = slug_id(&item.id);
    let manager_id = format!("team-{base}-manager");
    let member_id = format!("team-{base}-member");
    Ok(json!({
        "message": format!("宸插畨瑁呭洟闃燂細{}", item.name),
        "agents": [
            {
                "id": manager_id,
                "name": format!("{}缁忕悊", item.name.chars().take(3).collect::<String>()),
                "role": "manager",
                "title": "甯傚満鍥㈤槦缁忕悊",
                "specialty": item.description,
                "status": "online",
                "color": "#7c3aed",
                "sourceId": item.id
            },
            {
                "id": member_id,
                "name": format!("{}涓撳憳", item.name.chars().take(3).collect::<String>()),
                "role": "har",
                "title": "甯傚満鍥㈤槦鑱屽憳",
                "specialty": item.description,
                "status": "idle",
                "color": "#0891b2",
                "sourceId": item.id
            }
        ],
        "offices": [{
            "id": format!("office-{}", base),
            "name": item.name,
            "description": item.description,
            "managerIds": [format!("team-{base}-manager")],
            "memberIds": [format!("team-{base}-member")],
            "progress": 0,
            "sourceId": item.id
        }]
    }))
}

#[tauri::command]
fn install_market_skill(request: InstallRequest) -> Result<Value, String> {
    let item = request.item;
    Ok(json!({
        "message": format!("宸插畨瑁呮妧鑳斤細{}", item.name),
        "skills": [{
            "id": format!("skill-{}", slug_id(&item.id)),
            "name": item.name,
            "description": item.description,
            "enabled": true,
            "category": item.tags.as_ref().and_then(|tags| tags.first()).cloned().unwrap_or_else(|| "甯傚満".to_owned()),
            "source": item.source.unwrap_or_else(|| "openpisci".to_owned()),
            "sourceId": item.id,
            "tools": []
        }]
    }))
}

#[tauri::command]
fn install_default_skills(_app: AppHandle) -> Result<Value, String> {
    Ok(json!({
        "message": "Default skills installed.",
        "skills": [
            { "id": "skill-web-fetch", "name": "Web Fetch", "description": "Read public web pages for Agent research.", "enabled": true, "category": "research", "source": "builtin", "tools": ["web_fetch"] },
            { "id": "skill-file-write", "name": "Local File IO", "description": "Read, write and list files in the active workspace.", "enabled": true, "category": "file", "source": "builtin", "tools": ["write_text_file", "read_text_file", "list_files"] },
            { "id": "skill-shell-exec", "name": "Shell Exec", "description": "Run PowerShell or CMD commands in the active workspace.", "enabled": true, "category": "system", "source": "builtin", "tools": ["shell_exec"] },
            { "id": "skill-desktop-control", "name": "Desktop Control", "description": "Expose browser and computer control tool interfaces.", "enabled": true, "category": "automation", "source": "builtin", "tools": ["browser_control", "computer_control"] },
            { "id": "skill-news-xlsx", "name": "News Excel", "description": "Fetch news headlines and generate a real .xlsx file.", "enabled": true, "category": "office", "source": "builtin", "tools": ["news_to_excel"] },
            { "id": "skill-skill-search", "name": "Skill Search", "description": "Search enabled installed skills before Agent planning.", "enabled": true, "category": "runtime", "source": "builtin", "tools": ["skill_search", "skill_list"] },
            { "id": "skill-memory-md", "name": "MEMORY.md", "description": "Store, search, read and append long-term memory.", "enabled": true, "category": "memory", "source": "builtin", "tools": ["memory_search", "memory_read", "memory_append"] }
        ]
    }))
}

fn slug_id(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn builtin_tools() -> Vec<Value> {
    vec![
        json!({"id":"skill_search","name":"Skill Search","category":"runtime","description":"Search enabled skills before planning.","enabled":true}),
        json!({"id":"skill_read","name":"Skill Read","category":"runtime","description":"Read installed skill instructions.","enabled":true}),
        json!({"id":"skill_list","name":"Skill List","category":"runtime","description":"List enabled and disabled skills.","enabled":true}),
        json!({"id":"memory_search","name":"Memory Search","category":"memory","description":"Search MEMORY.md chunks.","enabled":true}),
        json!({"id":"memory_read","name":"Memory Read","category":"memory","description":"Read MEMORY.md.","enabled":true}),
        json!({"id":"memory_append","name":"Memory Append","category":"memory","description":"Append stable facts to MEMORY.md.","enabled":true}),
        json!({"id":"team_roster","name":"Team Roster","category":"team","description":"Read people, projects and task relationships.","enabled":true}),
        json!({"id":"news_to_excel","name":"News Excel","category":"office","description":"Fetch headlines and generate an Excel workbook.","enabled":true}),
        json!({"id":"web_fetch","name":"Web Fetch","category":"browser","description":"Read public web page text.","enabled":true}),
        json!({"id":"write_text_file","name":"Write Text File","category":"filesystem","description":"Write text files in workspace.","enabled":true}),
        json!({"id":"read_text_file","name":"Read Text File","category":"filesystem","description":"Read workspace or absolute text files.","enabled":true}),
        json!({"id":"list_files","name":"List Files","category":"filesystem","description":"List files in workspace or target directory.","enabled":true}),
        json!({"id":"shell_exec","name":"Shell Exec","category":"system","description":"Run PowerShell or CMD commands.","enabled":true}),
        json!({"id":"browser_control","name":"Browser Control","category":"browser","description":"Browser tool interface.","enabled":true}),
        json!({"id":"computer_control","name":"Computer Control","category":"desktop","description":"Desktop automation interface.","enabled":true}),
        json!({"id":"im_channel_status","name":"IM Channel Status","category":"im","description":"Read IM channel connection status.","enabled":true}),
    ]
}

#[tauri::command]
fn list_builtin_tools() -> Result<Vec<Value>, String> {
    Ok(builtin_tools())
}

#[tauri::command]
fn get_runtime_topology() -> Result<Value, String> {
    Ok(json!({
        "product": "Xiaoheng WorkBuddy",
        "runtime": "desktop",
        "marketplaceRoot": marketplace_root().to_string_lossy(),
        "skillMarkets": ["OpenPiscis", "clawhub", "skillhub", "OpenAI"],
        "modules": [
            {"id":"bootstrap","name":"Bootstrap","description":"Startup configuration and local state.","status":"ready"},
            {"id":"agent-tools","name":"Agent Tools","description":"Registered tools for model function calling.","status":"ready"},
            {"id":"marketplace","name":"Marketplace","description":"Skill and team marketplace integration.","status":"ready"},
            {"id":"team","name":"Team","description":"People, projects and tasks.","status":"ready"},
            {"id":"skills","name":"Skills","description":"Installed and enabled skills.","status":"ready"},
            {"id":"im","name":"IM Channels","description":"Messaging channel bridge status.","status":"partial"}
        ],
        "tools": builtin_tools()
    }))
}

fn default_im_channels() -> Vec<Value> {
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    let statuses = state.lock().ok();
    let configs = IM_CONFIG.get_or_init(|| Mutex::new(BTreeMap::new()));
    let config_values = configs.lock().ok();
    let make = |id: &str,
                name: &str,
                description: &str,
                capabilities: Vec<&str>,
                schema: Vec<Value>| {
        let configured = config_values
            .as_ref()
            .and_then(|map| map.get(id))
            .and_then(Value::as_object)
            .map(|obj| !obj.is_empty())
            .unwrap_or(false);
        let status = statuses
            .as_ref()
            .and_then(|map| map.get(id).cloned())
            .unwrap_or_else(|| {
                if configured {
                    "configured".to_owned()
                } else {
                    "disconnected".to_owned()
                }
            });
        json!({
            "id": id,
            "name": name,
            "description": description,
            "status": status,
            "capabilities": capabilities,
            "connectionMode": if id == "wechat" { "xiaoheng-weixin-native" } else { "config" },
            "setupHint": if id == "wechat" { "Click scan connect to show a WeChat QR code." } else { "" },
            "configSchema": schema,
            "config": config_values.as_ref().and_then(|map| map.get(id).cloned()).unwrap_or_else(|| json!({})),
            "updatedAt": now_string()
        })
    };
    vec![
        make(
            "wechat",
            "WeChat",
            "Native WeChat bridge using Xiaoheng runtime.",
            vec!["receive", "send", "qr-login"],
            vec![],
        ),
        make(
            "wecom",
            "WeCom",
            "WeCom app message and webhook channel.",
            vec!["send", "webhook"],
            vec![
                json!({"key":"botId","label":"Bot ID","required":true}),
                json!({"key":"botSecret","label":"Bot Secret","secret":true,"required":true}),
                json!({"key":"webhookUrl","label":"Group Webhook","required":false}),
            ],
        ),
        make(
            "feishu",
            "Feishu",
            "Feishu bot, app events and message sender.",
            vec!["send", "event"],
            vec![
                json!({"key":"appId","label":"App ID","required":true}),
                json!({"key":"appSecret","label":"App Secret","secret":true,"required":true}),
                json!({"key":"webhookUrl","label":"Bot Webhook","required":false}),
            ],
        ),
        make(
            "dingtalk",
            "DingTalk",
            "DingTalk bot and work notification channel.",
            vec!["send", "webhook"],
            vec![
                json!({"key":"appKey","label":"App Key","required":false}),
                json!({"key":"appSecret","label":"App Secret","secret":true,"required":false}),
                json!({"key":"webhookUrl","label":"Bot Webhook","required":false}),
            ],
        ),
        make(
            "telegram",
            "Telegram",
            "Telegram Bot API.",
            vec!["send", "receive"],
            vec![
                json!({"key":"botToken","label":"Bot Token","secret":true,"required":true}),
                json!({"key":"chatId","label":"Chat ID","required":true}),
            ],
        ),
        make(
            "slack",
            "Slack",
            "Slack app or webhook channel.",
            vec!["send", "event"],
            vec![json!({"key":"webhookUrl","label":"Incoming Webhook URL","required":true})],
        ),
        make(
            "discord",
            "Discord",
            "Discord bot or webhook channel.",
            vec!["send", "webhook"],
            vec![json!({"key":"webhookUrl","label":"Webhook URL","required":true})],
        ),
        make(
            "teams",
            "Microsoft Teams",
            "Teams incoming webhook.",
            vec!["send", "webhook"],
            vec![json!({"key":"webhookUrl","label":"Incoming Webhook URL","required":true})],
        ),
        make(
            "matrix",
            "Matrix",
            "Matrix room bridge.",
            vec!["send", "receive"],
            vec![
                json!({"key":"homeserver","label":"Homeserver","required":true,"placeholder":"https://matrix.org"}),
                json!({"key":"accessToken","label":"Access Token","secret":true,"required":true}),
                json!({"key":"roomId","label":"Room ID","required":true}),
            ],
        ),
        make(
            "webhook",
            "Webhook",
            "Generic HTTP webhook channel.",
            vec!["receive", "send"],
            vec![
                json!({"key":"outboundUrl","label":"Outbound URL","required":true}),
                json!({"key":"bearerToken","label":"Bearer Token","secret":true,"required":false}),
            ],
        ),
    ]
}

#[tauri::command]
fn list_im_channels() -> Result<Vec<Value>, String> {
    Ok(default_im_channels())
}

#[tauri::command]
fn connect_im_channel(channel_id: String) -> Result<Value, String> {
    validate_im_config(&channel_id)?;
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert(channel_id.clone(), "connected".to_owned());
    Ok(json!({"ok": true, "channelId": channel_id, "status": "connected"}))
}

#[tauri::command]
fn disconnect_im_channel(channel_id: String) -> Result<Value, String> {
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert(channel_id.clone(), "disconnected".to_owned());
    Ok(json!({"ok": true, "channelId": channel_id, "status": "disconnected"}))
}

#[tauri::command]
fn install_weixin_channel(app: AppHandle) -> Result<Value, String> {
    weixin_login_start(app, None)
}

fn weixin_state_file(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("im-weixin");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("account.json"))
}

fn weixin_account(app: &AppHandle) -> Option<Value> {
    weixin_state_file(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
}

fn save_weixin_account(app: &AppHandle, account: &Value) -> Result<(), String> {
    let path = weixin_state_file(app)?;
    fs::write(
        path,
        serde_json::to_string_pretty(account).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn weixin_sync_file(app: &AppHandle, account_id: &str) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("im-weixin");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{}.sync.json", safe_slug(account_id))))
}

fn load_weixin_sync_buf(app: &AppHandle, account_id: &str) -> String {
    weixin_sync_file(app, account_id)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["get_updates_buf"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn save_weixin_sync_buf(app: &AppHandle, account_id: &str, buf: &str) {
    if let Ok(path) = weixin_sync_file(app, account_id) {
        let _ = fs::write(
            path,
            serde_json::to_string(&json!({"get_updates_buf": buf})).unwrap_or_default(),
        );
    }
}

fn weixin_common_headers() -> BTreeMap<String, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned());
    let mut headers = BTreeMap::new();
    headers.insert("iLink-App-Id".to_owned(), WEIXIN_ILINK_APP_ID.to_owned());
    headers.insert(
        "iLink-App-ClientVersion".to_owned(),
        WEIXIN_ILINK_CLIENT_VERSION.to_owned(),
    );
    headers.insert(
        "X-WECHAT-UIN".to_owned(),
        BASE64_STANDARD.encode(millis.as_bytes()),
    );
    headers
}

fn weixin_base_info() -> Value {
    json!({
        "channel_version": "2.4.3",
        "bot_agent": "XiaohengWorkBuddy/0.1"
    })
}

fn text_from_weixin_message(message: &Value) -> String {
    message["item_list"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text_item"]["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn weixin_client_id() -> String {
    format!("xiaoheng-weixin-{}", now_string())
}

fn with_weixin_headers(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut builder = request;
    for (key, value) in weixin_common_headers() {
        builder = builder.header(key, value);
    }
    if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
        builder = builder
            .header("AuthorizationType", "ilink_bot_token")
            .bearer_auth(token.trim());
    }
    builder
}

#[tauri::command]
fn weixin_accounts(app: AppHandle) -> Result<Value, String> {
    Ok(json!({
        "connected": weixin_account(&app).is_some(),
        "account": weixin_account(&app).map(|account| json!({
            "accountId": account["accountId"].clone(),
            "baseUrl": account["baseUrl"].clone(),
            "userId": account["userId"].clone(),
            "updatedAt": account["updatedAt"].clone()
        }))
    }))
}

#[tauri::command]
fn weixin_login_start(app: AppHandle, force: Option<bool>) -> Result<Value, String> {
    if force != Some(true) {
        if let Some(account) = weixin_account(&app) {
            return Ok(json!({
                "ok": true,
                "alreadyConnected": true,
                "status": "connected",
                "accountId": account["accountId"],
                "message": "WeChat channel is connected. Rescan to refresh login state."
            }));
        }
    }
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert("wechat".to_owned(), "testing".to_owned());
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = weixin_fetch_qrcode(app_clone.clone()).await;
        match result {
            Ok(payload) => {
                let _ = app_clone.emit("xiaoheng://weixin-login", payload);
            }
            Err(error) => {
                let _ = app_clone.emit(
                    "xiaoheng://weixin-login",
                    json!({"status":"error","error":error}),
                );
            }
        }
    });
    Ok(json!({"ok": true, "status": "starting", "message": "Generating WeChat login QR code."}))
}

async fn weixin_fetch_qrcode(app: AppHandle) -> Result<Value, String> {
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "{WEIXIN_ILINK_BASE_URL}/ilink/bot/get_bot_qrcode?bot_type={WEIXIN_DEFAULT_BOT_TYPE}"
    );
    let local_token_list: Vec<String> = weixin_account(&app)
        .and_then(|account| {
            account["botToken"]
                .as_str()
                .map(|token| vec![token.to_owned()])
        })
        .unwrap_or_default();
    let response = with_weixin_headers(
        client
            .post(url)
            .json(&json!({"local_token_list": local_token_list})),
        None,
    )
    .send()
    .await
    .map_err(|e| format!("weixin qrcode request failed: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("weixin qrcode response parse failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("weixin qrcode request returned {status}: {body}"));
    }
    let qrcode = body["qrcode"]
        .as_str()
        .ok_or_else(|| "weixin qrcode response missing qrcode".to_owned())?
        .to_owned();
    let qrcode_url = body["qrcode_img_content"]
        .as_str()
        .ok_or_else(|| "weixin qrcode response missing qrcode_img_content".to_owned())?
        .to_owned();
    let session_key = format!("wx-{}", now_string());
    let session = WeixinLoginSession {
        qrcode,
        current_base_url: WEIXIN_ILINK_BASE_URL.to_owned(),
        started_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    };
    WEIXIN_LOGINS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "failed to lock weixin login state".to_owned())?
        .insert(session_key.clone(), session);
    Ok(json!({
        "status": "qrcode",
        "sessionKey": session_key,
        "qrcodeUrl": qrcode_url,
        "message": "Scan this QR code with WeChat and confirm on your phone."
    }))
}

#[tauri::command]
async fn weixin_login_poll(app: AppHandle, session_key: String) -> Result<Value, String> {
    let session = WEIXIN_LOGINS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "failed to lock weixin login state".to_owned())?
        .get(&session_key)
        .cloned()
        .ok_or_else(|| {
            "WeChat login session is missing or expired. Please scan again.".to_owned()
        })?;
    let age_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .saturating_sub(session.started_ms);
    if age_ms > 5 * 60 * 1000 {
        WEIXIN_LOGINS
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .map_err(|_| "failed to lock weixin login state".to_owned())?
            .remove(&session_key);
        return Ok(
            json!({"status": "expired", "message": "QR code expired. Please generate a new one."}),
        );
    }
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(38))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!(
        "{}/ilink/bot/get_qrcode_status?qrcode={}",
        session.current_base_url,
        percent_encode(&session.qrcode)
    );
    let response = with_weixin_headers(client.get(url), None)
        .send()
        .await
        .map_err(|e| format!("weixin qrcode status request failed: {e}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("weixin qrcode status response parse failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("weixin qrcode status returned {status}: {body}"));
    }
    let next_status = body["status"].as_str().unwrap_or("wait");
    if next_status == "scaned_but_redirect" {
        if let Some(host) = body["redirect_host"].as_str() {
            WEIXIN_LOGINS
                .get_or_init(|| Mutex::new(BTreeMap::new()))
                .lock()
                .map_err(|_| "failed to lock weixin login state".to_owned())?
                .entry(session_key.clone())
                .and_modify(|item| item.current_base_url = format!("https://{host}"));
        }
    }
    if next_status == "confirmed" {
        let account_id = body["ilink_bot_id"].as_str().ok_or_else(|| {
            "WeChat login confirmed but response is missing ilink_bot_id.".to_owned()
        })?;
        let account = json!({
            "accountId": account_id,
            "botToken": body["bot_token"].as_str().unwrap_or_default(),
            "baseUrl": body["baseurl"].as_str().unwrap_or(WEIXIN_ILINK_BASE_URL),
            "userId": body["ilink_user_id"].as_str().unwrap_or_default(),
            "updatedAt": now_string()
        });
        save_weixin_account(&app, &account)?;
        start_weixin_monitor(app.clone())?;
        WEIXIN_LOGINS
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .map_err(|_| "failed to lock weixin login state".to_owned())?
            .remove(&session_key);
        let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
        state
            .lock()
            .map_err(|_| "failed to lock IM state".to_owned())?
            .insert("wechat".to_owned(), "connected".to_owned());
        return Ok(
            json!({"status": "connected", "accountId": account_id, "message": "WeChat login connected."}),
        );
    }
    if next_status == "binded_redirect" {
        let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
        state
            .lock()
            .map_err(|_| "failed to lock IM state".to_owned())?
            .insert("wechat".to_owned(), "connected".to_owned());
        return Ok(
            json!({"status": "connected", "message": "This WeChat account is already bound to this WorkBuddy instance."}),
        );
    }
    Ok(json!({
        "status": next_status,
        "message": match next_status {
            "scaned" => "Scanned. Please confirm on your phone.",
            "wait" => "Waiting for scan.",
            "expired" => "QR code expired. Please generate a new one.",
            "need_verifycode" => "Phone verification code is required.",
            _ => "Waiting for WeChat confirmation."
        },
        "raw": body
    }))
}

#[tauri::command]
fn weixin_monitor_start(app: AppHandle) -> Result<Value, String> {
    if weixin_account(&app).is_none() {
        return Err("WeChat is not connected. Please complete QR login first.".to_owned());
    }
    start_weixin_monitor(app.clone())?;
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert("wechat".to_owned(), "connected".to_owned());
    Ok(
        json!({"ok": true, "channelId": "wechat", "status": "connected", "message": "WeChat account is connected and message monitoring has started."}),
    )
}

fn start_weixin_monitor(app: AppHandle) -> Result<(), String> {
    let account = weixin_account(&app).ok_or_else(|| "WeChat is not connected.".to_owned())?;
    let account_id = account["accountId"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "WeChat account is missing accountId.".to_owned())?
        .to_owned();
    let inserted = {
        let mut monitors = WEIXIN_MONITORS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
            .map_err(|_| "failed to lock weixin monitor state".to_owned())?;
        monitors.insert(account_id.clone())
    };
    if !inserted {
        return Ok(());
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = weixin_monitor_loop(app.clone(), account_id.clone()).await {
            let _ = app.emit(
                "xiaoheng://im-status",
                json!({"channel":"wechat","status":"error","error":error}),
            );
        }
        if let Ok(mut monitors) = WEIXIN_MONITORS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
        {
            monitors.remove(&account_id);
        }
    });
    Ok(())
}

async fn weixin_monitor_loop(app: AppHandle, account_id: String) -> Result<(), String> {
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(45))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let mut failures = 0_u32;
    loop {
        let Some(account) = weixin_account(&app) else {
            return Ok(());
        };
        let token = account["botToken"].as_str().unwrap_or_default().to_owned();
        let base_url = account["baseUrl"]
            .as_str()
            .unwrap_or(WEIXIN_ILINK_BASE_URL)
            .to_owned();
        let mut sync_buf = load_weixin_sync_buf(&app, &account_id);
        let url = format!("{base_url}/ilink/bot/getupdates");
        let response = with_weixin_headers(
            client.post(url).json(&json!({
                "get_updates_buf": sync_buf,
                "base_info": weixin_base_info()
            })),
            Some(&token),
        )
        .send()
        .await;
        let response = match response {
            Ok(value) => value,
            Err(error) => {
                failures += 1;
                let _ = app.emit(
                    "xiaoheng://im-status",
                    json!({"channel":"wechat","status":"retrying","error":error.to_string()}),
                );
                tokio::time::sleep(std::time::Duration::from_secs(if failures > 3 {
                    30
                } else {
                    2
                }))
                .await;
                continue;
            }
        };
        failures = 0;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("weixin getupdates response parse failed: {e}"))?;
        if !status.is_success() {
            let _ = app.emit("xiaoheng://im-status", json!({"channel":"wechat","status":"retrying","error":format!("weixin getupdates returned {status}: {body}")}));
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        }
        if let Some(next_buf) = body["get_updates_buf"].as_str() {
            sync_buf = next_buf.to_owned();
            save_weixin_sync_buf(&app, &account_id, &sync_buf);
        }
        if let Some(messages) = body["msgs"].as_array() {
            for message in messages {
                let text = text_from_weixin_message(message);
                let from_user_id = message["from_user_id"].as_str().unwrap_or_default();
                if text.is_empty() || from_user_id.is_empty() {
                    continue;
                }
                let _ = app.emit(
                    "xiaoheng://im-inbound",
                    json!({
                        "channel": "wechat",
                        "accountId": account_id,
                        "fromUserId": from_user_id,
                        "messageId": message["message_id"].clone(),
                        "sessionId": message["session_id"].clone(),
                        "contextToken": message["context_token"].clone(),
                        "content": text,
                        "timestamp": now_string()
                    }),
                );
            }
        }
    }
}

#[tauri::command]
async fn weixin_send_text(
    app: AppHandle,
    to_user_id: String,
    content: String,
    context_token: Option<String>,
    settings: Option<Value>,
) -> Result<Value, String> {
    let account = weixin_account(&app).ok_or_else(|| "WeChat is not connected.".to_owned())?;
    let token = account["botToken"].as_str().unwrap_or_default();
    let base_url = account["baseUrl"].as_str().unwrap_or(WEIXIN_ILINK_BASE_URL);
    if to_user_id.trim().is_empty() {
        return Err("Missing WeChat receiver ilink_user_id.".to_owned());
    }
    let client = apply_configured_proxy(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Xiaoheng-WorkBuddy/0.1")
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{base_url}/ilink/bot/sendmessage");
    let client_id = weixin_client_id();
    let content = decorate_im_content_with_file_links("wechat", &content, settings.as_ref()).await;
    let payload = json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": to_user_id,
            "client_id": client_id,
            "message_type": 2,
            "message_state": 2,
            "context_token": context_token
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            "item_list": [{
                "type": 1,
                "text_item": { "text": content }
            }]
        },
        "base_info": weixin_base_info()
    });
    let response = with_weixin_headers(client.post(url).json(&payload), Some(token))
        .send()
        .await
        .map_err(|e| format!("weixin sendmessage request failed: {e}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("weixin sendmessage returned {status}: {body}"));
    }
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!({"raw": body}));
    Ok(json!({"ok": true, "status": "sent", "messageId": client_id, "raw": parsed}))
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn emit_tray_command(app: &AppHandle, command: &str) {
    show_main_window(app);
    let _ = app.emit("xiaoheng://tray-command", json!({ "command": command }));
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("new-chat", "New Chat")
        .text("settings", "Settings")
        .separator()
        .text("show", "Open")
        .text("quit", "Quit")
        .build()?;
    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "missing default window icon")
    })?;
    TrayIconBuilder::with_id("xiaoheng-tray")
        .icon(icon)
        .tooltip("Xiaoheng WorkBuddy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main_window(&tray.app_handle());
            }
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tray.set_tooltip(Some("Xiaoheng WorkBuddy"));
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "new-chat" => emit_tray_command(app, "new-chat"),
            "settings" => emit_tray_command(app, "settings"),
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn tray_notify_attention(app: AppHandle, title: String) -> Result<(), String> {
    if let Some(tray) = app.tray_by_id("xiaoheng-tray") {
        let _ = tray.set_tooltip(Some(format!("Xiaoheng has a new reply: {title}")));
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.request_user_attention(Some(UserAttentionType::Informational));
    }
    Ok(())
}

fn config_string<'a>(config: &'a Value, key: &str) -> &'a str {
    config.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

fn validate_im_config(channel_id: &str) -> Result<Value, String> {
    let configs = IM_CONFIG.get_or_init(|| Mutex::new(BTreeMap::new()));
    let config = configs
        .lock()
        .map_err(|_| "failed to lock IM config".to_owned())?
        .get(channel_id)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let has = |key: &str| !config_string(&config, key).is_empty();
    let ok = match channel_id {
        "wechat" => true,
        "wecom" => has("webhookUrl") || (has("botId") && has("botSecret")),
        "feishu" => has("webhookUrl") || (has("appId") && has("appSecret")),
        "dingtalk" => has("webhookUrl") || (has("appKey") && has("appSecret")),
        "telegram" => has("botToken") && has("chatId"),
        "slack" | "discord" | "teams" => has("webhookUrl"),
        "matrix" => has("homeserver") && has("accessToken") && has("roomId"),
        "webhook" => has("outboundUrl"),
        _ => false,
    };
    if ok {
        Ok(config)
    } else {
        Err("Please fill and save the required IM channel configuration first. The channel will not connect to Agent until validation passes.".to_owned())
    }
}

#[tauri::command]
fn save_im_channel_config(request: ImConfigRequest) -> Result<Value, String> {
    let configs = IM_CONFIG.get_or_init(|| Mutex::new(BTreeMap::new()));
    configs
        .lock()
        .map_err(|_| "failed to lock IM config".to_owned())?
        .insert(request.channel_id.clone(), request.config);
    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert(request.channel_id.clone(), "configured".to_owned());
    Ok(json!({"ok": true, "channelId": request.channel_id, "status": "configured"}))
}

#[tauri::command]
async fn test_im_channel(
    app: AppHandle,
    channel_id: String,
    content: Option<String>,
) -> Result<Value, String> {
    if channel_id == "wechat" {
        if weixin_account(&app).is_none() {
            return Err("WeChat is not connected. Please click scan login first.".to_owned());
        }
        let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
        state
            .lock()
            .map_err(|_| "failed to lock IM state".to_owned())?
            .insert(channel_id.clone(), "connected".to_owned());
        return Ok(json!({"ok": true, "channelId": channel_id, "status": "connected"}));
    }
    let config = validate_im_config(&channel_id)?;
    let text = content.unwrap_or_else(|| "Xiaoheng WorkBuddy IM test message".to_owned());
    let webhook_url = config
        .get("webhookUrl")
        .or_else(|| config.get("outboundUrl"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    if let Some(url) = webhook_url {
        let client = apply_configured_proxy(reqwest::Client::builder())
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;
        let payload = match channel_id.as_str() {
            "discord" => json!({"content": text}),
            "slack" => json!({"text": text}),
            "teams" => json!({"text": text}),
            "dingtalk" | "feishu" | "wecom" => {
                json!({"msg_type":"text","msgtype":"text","text":{"content":text},"content":text})
            }
            _ => json!({"text": text, "source": "xiaoheng-workbuddy"}),
        };
        let mut request = client.post(url).json(&payload);
        if let Some(token) = config.get("bearerToken").and_then(Value::as_str) {
            if !token.trim().is_empty() {
                request = request.bearer_auth(token.trim());
            }
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("failed to send test message: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("IM test returned {status}: {body}"));
        }
    }

    let state = IM_STATE.get_or_init(|| Mutex::new(BTreeMap::new()));
    state
        .lock()
        .map_err(|_| "failed to lock IM state".to_owned())?
        .insert(channel_id.clone(), "connected".to_owned());
    Ok(json!({"ok": true, "channelId": channel_id, "status": "connected"}))
}

fn latest_user_prompt_from_context(work_context: &Value) -> String {
    work_context
        .get("activeConversation")
        .and_then(|conversation| conversation.get("messages"))
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().rev().find_map(|message| {
                let role = message.get("role").and_then(Value::as_str)?;
                if role == "user" {
                    message
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn artifact_instruction(prompt: &str) -> &'static str {
    let lower = prompt.to_lowercase();
    let asks_file = lower.contains("ppt")
        || lower.contains("pptx")
        || lower.contains("ppf")
        || lower.contains("powerpoint")
        || lower.contains("excel")
        || lower.contains("xlsx")
        || lower.contains("word")
        || lower.contains("docx")
        || lower.contains("pdf")
        || lower.contains("markdown")
        || lower.contains(".md")
        || prompt.contains("保存")
        || prompt.contains("导出")
        || prompt.contains("生成");
    if asks_file {
        "产物任务硬规则：用户要文件/导出/保存时，先确定输出路径和文件名；不要反复口头汇报或询问是否继续；最多一次读取源材料后必须进入生成步骤；必须用工具创建文件并用 list_files/read_text_file/shell_exec 等验证文件存在、大小合理后，才允许回复完成。若工具结果出现 contentTruncated/stdoutTruncated/stderrTruncated=true，只代表返回给模型或界面的预览被截断，不能据此判断文件为空、脚本缺内容或命令失败；必须以 ok/status、byteSize、charCount、contentPreview、contentTail、list_files 条目和实际产物文件为准。若当前会话权限阻止写文件、Shell 或工作区外访问，直接请求用户把当前会话权限切到“完全访问”，不要继续空转。"
    } else {
        "普通任务规则：优先直接回答或执行最短必要工具链；不要把无关历史任务、其它项目或其它文件混入当前任务。"
    }
}

fn summarize_work_context(work_context: &Value) -> String {
    let coordinator = work_context
        .get("coordinator")
        .cloned()
        .unwrap_or(Value::Null);
    let coordinator_name = coordinator
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("小亨");
    let coordinator_title = coordinator
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("主协调 Agent");
    let coordinator_specialty = coordinator
        .get("specialty")
        .and_then(Value::as_str)
        .unwrap_or("");
    let options = work_context
        .get("activeConversation")
        .and_then(|value| value.get("options"))
        .cloned()
        .unwrap_or(Value::Null);
    let workspace_root = options
        .get("workspaceRoot")
        .and_then(Value::as_str)
        .unwrap_or("");
    let office_id = options
        .get("officeId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let expert_id = options
        .get("expertId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let model = options.get("model").and_then(Value::as_str).unwrap_or("");
    let counts = (
        work_context
            .get("roster")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
        work_context
            .get("offices")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
        work_context
            .get("tasks")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
        work_context
            .get("enabledSkills")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
        work_context
            .get("memories")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
        work_context
            .get("schedules")
            .and_then(Value::as_array)
            .map(|value| value.len())
            .unwrap_or(0),
    );
    let enabled_skills = work_context
        .get("enabledSkills")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|skill| skill.get("name").and_then(Value::as_str))
                .take(10)
                .collect::<Vec<_>>()
                .join("、")
        })
        .unwrap_or_default();
    let _tasks = work_context
        .get("tasks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|task| {
                    let title = task.get("title").and_then(Value::as_str)?;
                    let status = task.get("status").and_then(Value::as_str).unwrap_or("todo");
                    let owner = task
                        .get("owner")
                        .and_then(|value| value.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("未指派");
                    Some(format!("{status}:{title}({owner})"))
                })
                .take(10)
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();
    let _offices = work_context
        .get("offices")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|office| {
                    let name = office.get("name").and_then(Value::as_str)?;
                    let progress = office.get("progress").and_then(Value::as_u64).unwrap_or(0);
                    Some(format!("{name}({progress}%)"))
                })
                .take(6)
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();
    let latest_prompt = latest_user_prompt_from_context(work_context);
    let generated_at = context_generated_at(work_context).unwrap_or_else(|| now_string());
    let beijing_time =
        add_hours_to_utc_iso(&generated_at, 8).unwrap_or_else(|| generated_at.clone());
    let artifact_rule = artifact_instruction(&latest_prompt);
    let permission_mode = session_permission_mode(work_context);
    let skill_hints = triggered_skill_hints(&latest_prompt);
    let dispatch_hints = triggered_dispatch_hints(&latest_prompt);
    let attachments = work_context
        .get("attachments")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("id").and_then(Value::as_str)?;
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("attachment");
                    let mime = item.get("mime").and_then(Value::as_str).unwrap_or("");
                    Some(format!("{id}:{name}({mime})"))
                })
                .take(8)
                .collect::<Vec<_>>()
                .join("；")
        })
        .unwrap_or_default();

    format!(
        "你是小亨 WorkBuddy Gateway，当前人格/职责：{coordinator_name}（{coordinator_title}），{coordinator_specialty}。\n\
你采用 OpenClaw Gateway 式执行：模型负责理解、决策、写代码和组织内容；Gateway 工具负责读写文件、执行命令、联网和验证结果。\n\
对话配置：workspaceRoot={workspace_root}；officeId={office_id}；expertId={expert_id}；model={model}。\n\
当前会话权限：{permission_mode}（restricted=受限，approval=请求批准，full=完全访问）。需要 Shell、文件写入或工作区外访问时，只有 full 会执行；approval/restricted 必须请求用户提升当前会话权限，不要假装已经执行。\n\
当前时间：UTC/ISO={generated_at}；北京时间={beijing_time}。涉及“今天/现在/最近”必须以这里或真实工具返回为准，不得凭模型记忆或伪代码猜时间。\n\
工作池摘要：成员 {0}，项目 {1}，任务 {2}，启用技能 {3}，记忆 {4}，自动化 {5}。启用技能摘要：{enabled_skills}。\n\
当前会话附件：{attachments}\n\
技能触发提示：{skill_hints}\n\
调度触发提示：{dispatch_hints}\n\
执行收敛规则：{artifact_rule}\n\
Gateway 工作原则：\n\
1. 简单问候、普通问答、解释类问题直接回复，不创建 todo，不调用工具。\n\
2. 只有需要真实文件、Shell、网页、桌面、项目/任务状态变更时才调用工具；工具调用要少而准。\n\
3. 文件制品任务按 OpenClaw 链路执行：确认路径和依赖 -> 一次性写完整脚本/文件 -> 执行 -> 验证大小/页数/内容 -> 必要时清理临时文件 -> 最终汇报。\n\
4. 不要把“规划、读取、总结”当成交付；不要反复询问是否继续；不要反复分段读取大文件，改用脚本提取摘要或直接生成。\n\
5. 普通问答不要创建任务中心 todo；但用户明确要求调度、分配、分别执行、多人协作、任务空间/任务中心可视化或指定人员执行时，必须先调用 team_roster，再用 task_plan_update/work_pool_update 同步 todo、负责人和状态。\n\
6. 用户只是查询/列出/检查技能、记忆、设置、任务或状态时，只读回答或调用只读工具；不得创建 Workspace、目录、任务、项目或初始化任何文件结构，除非用户明确要求初始化/创建。\n\
7. 始终用简体中文，最终回复只报告真实完成的结果和路径。",
        counts.0,
        counts.1,
        counts.2,
        counts.3,
        counts.4,
        counts.5
    )
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn strip_html_tags(input: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    html_unescape(output.trim()).trim().to_owned()
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    if let Some(start) = tag.find(&needle) {
        let rest = &tag[start + needle.len()..];
        let end = rest.find('"')?;
        return Some(html_unescape(&rest[..end]));
    }
    let needle = format!("{attr}='");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('\'')?;
    Some(html_unescape(&rest[..end]))
}

fn extract_news_items(html: &str, limit: usize, source_name: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for segment in html.split("<a ").skip(1) {
        let Some(end) = segment.find("</a>") else {
            continue;
        };
        let tag_and_text = &segment[..end];
        let Some(gt) = tag_and_text.find('>') else {
            continue;
        };
        let tag = &tag_and_text[..gt];
        let title = strip_html_tags(&tag_and_text[gt + 1..]);
        if title.chars().count() < 6 || title == source_name || title.contains("棣栭〉") {
            continue;
        }
        let url = extract_attr(tag, "href").unwrap_or_default();
        if url.is_empty() || !seen.insert(title.clone()) {
            continue;
        }
        rows.push(vec![
            (rows.len() + 1).to_string(),
            title,
            url,
            source_name.to_owned(),
        ]);
        if rows.len() >= limit {
            break;
        }
    }
    if rows.len() < limit {
        for segment in html.split("\"title\"").skip(1) {
            let Some(title_start) = segment.find('"') else {
                continue;
            };
            let rest = &segment[title_start + 1..];
            let Some(title_end) = rest.find('"') else {
                continue;
            };
            let title = html_unescape(&rest[..title_end].replace("\\\"", "\""));
            if title.chars().count() < 6
                || title.contains(source_name)
                || title.contains("advertisement")
            {
                continue;
            }
            let mut url = String::new();
            for key in ["\"url\"", "\"href\"", "\"link\""] {
                if let Some(key_pos) = segment.find(key) {
                    let after_key = &segment[key_pos + key.len()..];
                    if let Some(value_start) = after_key.find('"') {
                        let after_quote = &after_key[value_start + 1..];
                        if let Some(value_end) = after_quote.find('"') {
                            url = html_unescape(&after_quote[..value_end].replace("\\/", "/"));
                            break;
                        }
                    }
                }
            }
            if !url.starts_with("http") {
                continue;
            }
            if !seen.insert(title.clone()) {
                continue;
            }
            rows.push(vec![
                (rows.len() + 1).to_string(),
                title,
                url,
                source_name.to_owned(),
            ]);
            if rows.len() >= limit {
                break;
            }
        }
    }
    rows
}

fn worksheet_xml(rows: &[Vec<String>]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for (row_idx, row) in rows.iter().enumerate() {
        xml.push_str(&format!(r#"<row r="{}">"#, row_idx + 1));
        for (col_idx, cell) in row.iter().enumerate() {
            let col = ((b'A' + col_idx as u8) as char).to_string();
            xml.push_str(&format!(
                r#"<c r="{}{}" t="inlineStr"><is><t>{}</t></is></c>"#,
                col,
                row_idx + 1,
                xml_escape(cell)
            ));
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
}

fn write_zip_file(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    content: &str,
) -> Result<(), String> {
    zip.start_file(name, SimpleFileOptions::default())
        .map_err(|e| e.to_string())?;
    zip.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

fn write_xlsx(path: &Path, rows: &[Vec<String>]) -> Result<(), String> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    write_zip_file(
        &mut zip,
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
    )?;
    write_zip_file(
        &mut zip,
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
    )?;
    write_zip_file(
        &mut zip,
        "xl/workbook.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="BaiduNews" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
    )?;
    write_zip_file(
        &mut zip,
        "xl/_rels/workbook.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#,
    )?;
    write_zip_file(&mut zip, "xl/worksheets/sheet1.xml", &worksheet_xml(rows))?;
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    fs::write(path, bytes).map_err(|e| format!("failed to write xlsx: {e}"))
}

async fn news_to_excel(
    client: &reqwest::Client,
    args: &Value,
    work_context: &Value,
) -> Result<Value, String> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(3, 50) as usize;
    let source_name = args
        .get("sourceName")
        .or_else(|| args.get("source_name"))
        .and_then(Value::as_str)
        .unwrap_or("News Source");
    let source_url = args
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("http"))
        .unwrap_or("https://news.baidu.com/");
    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("news-headlines.xlsx");
    let html = client
        .get(source_url)
        .header("user-agent", "Mozilla/5.0 Xiaoheng WorkBuddy")
        .send()
        .await
        .map_err(|e| format!("failed to fetch news source: {e}"))?
        .text()
        .await
        .map_err(|e| format!("failed to read news response: {e}"))?;
    let mut rows = vec![vec![
        "index".to_owned(),
        "title".to_owned(),
        "url".to_owned(),
        "source".to_owned(),
    ]];
    rows.extend(extract_news_items(&html, limit, source_name));
    if rows.len() <= 1 {
        return Err(format!(
            "No headlines parsed from {source_name}. Try another news URL or retry later."
        ));
    }
    let path = active_workspace_dir(work_context)?.join(filename);
    write_xlsx(&path, &rows)?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "count": rows.len() - 1,
        "rows": rows,
        "source": source_url
    }))
}

async fn web_fetch(client: &reqwest::Client, args: &Value) -> Result<Value, String> {
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing url".to_owned())?;
    let text = client
        .get(url)
        .header("user-agent", "Mozilla/5.0 Xiaoheng WorkBuddy")
        .send()
        .await
        .map_err(|e| format!("failed to fetch url: {e}"))?
        .text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;
    let clean = strip_html_tags(&text);
    Ok(json!({
        "ok": true,
        "url": url,
        "text": clean.chars().take(12000).collect::<String>()
    }))
}

fn safe_output_path(args: &Value, work_context: &Value) -> Result<PathBuf, String> {
    if setting_bool(work_context, "confirmFileWrite", true) {
        return Err(permission_required_message("文件写入"));
    }
    if let Some(path) = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            active_workspace_dir(work_context)?.join(path)
        };
        let path = resolve_tool_path(path, work_context)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output directory: {e}"))?;
        }
        return Ok(path);
    }
    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("xiaoheng-output.txt")
        .to_owned();
    let filename = safe_filename(&filename);
    let base = args
        .get("directory")
        .or_else(|| args.get("dir"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(active_workspace_dir(work_context)?);
    let base = resolve_tool_path(base, work_context)?;
    fs::create_dir_all(&base).map_err(|e| format!("failed to create output directory: {e}"))?;
    Ok(base.join(filename))
}

fn safe_filename(filename: &str) -> String {
    let cleaned = filename
        .trim()
        .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    if cleaned.is_empty() {
        "xiaoheng-output.txt".to_owned()
    } else {
        cleaned
    }
}

fn mime_for_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "log" | "md" | "csv" | "json" | "yaml" | "yml" | "toml" | "rs" | "ts" | "tsx"
        | "js" | "jsx" | "py" | "ps1" | "bat" | "cmd" | "html" | "css" | "xml" => "text/plain",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn object_setting_string(settings: Option<&Value>, key: &str, default: &str) -> String {
    settings
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .to_owned()
}

fn object_setting_bool(settings: Option<&Value>, key: &str, default: bool) -> bool {
    settings
        .and_then(|value| value.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn object_setting_u64(settings: Option<&Value>, key: &str, default: u64) -> u64 {
    settings
        .and_then(|value| value.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(default)
}

fn object_storage_config(settings: Option<&Value>) -> ObjectStorageConfig {
    ObjectStorageConfig {
        enabled: object_setting_bool(settings, "objectStorageEnabled", true),
        endpoint: object_setting_string(
            settings,
            "objectStorageEndpoint",
            DEFAULT_OBJECT_STORAGE_ENDPOINT,
        ),
        upload_endpoint: object_setting_string(
            settings,
            "objectStorageUploadEndpoint",
            DEFAULT_OBJECT_STORAGE_UPLOAD_ENDPOINT,
        ),
        bucket: object_setting_string(
            settings,
            "objectStorageBucket",
            DEFAULT_OBJECT_STORAGE_BUCKET,
        ),
        access_key: object_setting_string(
            settings,
            "objectStorageAccessKey",
            DEFAULT_OBJECT_STORAGE_ACCESS_KEY,
        ),
        secret_key: object_setting_string(
            settings,
            "objectStorageSecretKey",
            DEFAULT_OBJECT_STORAGE_SECRET_KEY,
        ),
        region: object_setting_string(
            settings,
            "objectStorageRegion",
            DEFAULT_OBJECT_STORAGE_REGION,
        ),
        link_expiry_hours: object_setting_u64(settings, "objectStorageLinkExpiryHours", 24)
            .clamp(1, 24 * 30),
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn hmac_sha256(key: &[u8], value: &str) -> Result<Vec<u8>, String> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|e| e.to_string())?;
    mac.update(value.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

fn aws_signing_key(secret: &str, date: &str, region: &str) -> Result<Vec<u8>, String> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date)?;
    let k_region = hmac_sha256(&k_date, region)?;
    let k_service = hmac_sha256(&k_region, "s3")?;
    hmac_sha256(&k_service, "aws4_request")
}

fn aws_uri_encode(value: &str, encode_slash: bool) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            output.push(ch);
        } else if ch == '/' && !encode_slash {
            output.push('/');
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn endpoint_host(endpoint: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(endpoint).map_err(|e| format!("invalid MinIO endpoint: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "MinIO endpoint is missing host".to_owned())?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn object_url(endpoint: &str, bucket: &str, key: &str) -> Result<(String, String, String), String> {
    let endpoint = endpoint.trim_end_matches('/');
    reqwest::Url::parse(endpoint).map_err(|e| format!("invalid MinIO endpoint: {e}"))?;
    let encoded_key = aws_uri_encode(key, false);
    let canonical_uri = format!("/{}/{}", aws_uri_encode(bucket, true), encoded_key);
    Ok((
        format!("{endpoint}{canonical_uri}"),
        canonical_uri,
        endpoint_host(endpoint)?,
    ))
}

fn canonical_query(params: &[(String, String)]) -> String {
    let mut encoded = params
        .iter()
        .map(|(key, value)| (aws_uri_encode(key, true), aws_uri_encode(value, true)))
        .collect::<Vec<_>>();
    encoded.sort();
    encoded
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

fn signed_s3_authorization(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    host: &str,
    payload_hash: &str,
    config: &ObjectStorageConfig,
    amz_date: &str,
    date_stamp: &str,
) -> Result<String, String> {
    let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", config.region);
    let canonical_headers =
        format!("host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n");
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let request_hash = bytes_hex(&Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{request_hash}");
    let signing_key = aws_signing_key(&config.secret_key, date_stamp, &config.region)?;
    let signature = bytes_hex(&hmac_sha256(&signing_key, &string_to_sign)?);
    Ok(format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
        config.access_key
    ))
}

async fn minio_put_object(
    client: &reqwest::Client,
    config: &ObjectStorageConfig,
    key: &str,
    bytes: Vec<u8>,
    mime: &str,
) -> Result<(), String> {
    let (url, canonical_uri, host) = object_url(&config.upload_endpoint, &config.bucket, key)?;
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = bytes_hex(&Sha256::digest(&bytes));
    let authorization = signed_s3_authorization(
        "PUT",
        &canonical_uri,
        "",
        &host,
        &payload_hash,
        config,
        &amz_date,
        &date_stamp,
    )?;
    let response = client
        .put(url)
        .header("authorization", authorization)
        .header("x-amz-date", amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("content-type", mime)
        .body(bytes)
        .send()
        .await
        .map_err(|e| format!("MinIO upload request failed: {e}"))?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("MinIO upload returned {status}: {body}"))
    }
}

fn minio_presigned_get_url(config: &ObjectStorageConfig, key: &str) -> Result<String, String> {
    let (url, canonical_uri, host) = object_url(&config.endpoint, &config.bucket, key)?;
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let credential_scope = format!("{date_stamp}/{}/s3/aws4_request", config.region);
    let expires = (config.link_expiry_hours * 3600).to_string();
    let params = vec![
        ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
        (
            "X-Amz-Credential".to_owned(),
            format!("{}/{}", config.access_key, credential_scope),
        ),
        ("X-Amz-Date".to_owned(), amz_date.clone()),
        ("X-Amz-Expires".to_owned(), expires),
        ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
    ];
    let query = canonical_query(&params);
    let canonical_request =
        format!("GET\n{canonical_uri}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
    let request_hash = bytes_hex(&Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign =
        format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{request_hash}");
    let signing_key = aws_signing_key(&config.secret_key, &date_stamp, &config.region)?;
    let signature = bytes_hex(&hmac_sha256(&signing_key, &string_to_sign)?);
    Ok(format!("{url}?{query}&X-Amz-Signature={signature}"))
}

fn trim_path_token(value: &str) -> String {
    value
        .trim_matches(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '`' | '\''
                        | '"'
                        | '>'
                        | '<'
                        | ')'
                        | ']'
                        | '}'
                        | ','
                        | ';'
                        | '，'
                        | '。'
                        | '；'
                        | '）'
                        | '】'
                )
        })
        .to_owned()
}

fn extract_windows_file_paths(content: &str) -> Vec<PathBuf> {
    let pattern = WINDOWS_PATH_RE.get_or_init(|| {
        Regex::new(r#"(?i)[A-Z]:\\[^\r\n`"'<>|]+?\.[A-Za-z0-9]{1,12}"#).expect("valid path regex")
    });
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    for found in pattern.find_iter(content) {
        let cleaned = trim_path_token(found.as_str());
        let path = PathBuf::from(&cleaned);
        if path.is_file() {
            let key = path.to_string_lossy().to_string();
            if seen.insert(key) {
                paths.push(path);
            }
        }
    }
    paths
}

async fn decorate_im_content_with_file_links(
    channel: &str,
    content: &str,
    settings: Option<&Value>,
) -> String {
    let config = object_storage_config(settings);
    if !config.enabled {
        return content.to_owned();
    }
    let paths = extract_windows_file_paths(content);
    if paths.is_empty() {
        return content.to_owned();
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .build()
    {
        Ok(value) => value,
        Err(error) => return format!("{content}\n\n[文件链接生成失败] HTTP client: {error}"),
    };
    let now = Utc::now();
    let yyyy = now.format("%Y").to_string();
    let mm = now.format("%m").to_string();
    let mut cards = Vec::new();
    for path in paths {
        let bytes = match fs::read(&path) {
            Ok(value) => value,
            Err(error) => {
                cards.push(format!(
                    "文件读取失败：{} ({error})",
                    path.to_string_lossy()
                ));
                continue;
            }
        };
        let sha256 = bytes_hex(&Sha256::digest(&bytes));
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(safe_filename)
            .unwrap_or_else(|| "workbuddy-file".to_owned());
        let mime = mime_for_path(&path);
        let key = format!("workbuddy/{channel}/{yyyy}/{mm}/{sha256}/{name}");
        match minio_put_object(&client, &config, &key, bytes, &mime).await {
            Ok(()) => match minio_presigned_get_url(&config, &key) {
                Ok(url) => cards.push(format!(
                    "文件：{name}\n类型：{mime}\n大小：{} bytes\n链接：{url}\n有效期：{} 小时",
                    fs::metadata(&path)
                        .map(|meta| meta.len())
                        .unwrap_or_default(),
                    config.link_expiry_hours
                )),
                Err(error) => cards.push(format!("文件链接生成失败：{name} ({error})")),
            },
            Err(error) => cards.push(format!("文件上传失败：{name} ({error})")),
        }
    }
    if cards.is_empty() {
        content.to_owned()
    } else {
        format!(
            "{content}\n\n---\nWorkBuddy 文件卡片\n{}",
            cards.join("\n\n")
        )
    }
}

#[tauri::command]
fn register_attachment(app: AppHandle, path: String) -> Result<AttachmentItem, String> {
    let source_path = PathBuf::from(path.trim());
    if !source_path.is_file() {
        return Err("attachment path is not a file".to_owned());
    }
    let bytes = fs::read(&source_path).map_err(|e| format!("failed to read attachment: {e}"))?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(safe_filename)
        .unwrap_or_else(|| "attachment".to_owned());
    let cache_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?
        .join("attachments")
        .join(&sha256);
    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("failed to create attachment cache dir: {e}"))?;
    let cache_path = cache_dir.join(&name);
    if !cache_path.exists() {
        fs::write(&cache_path, &bytes).map_err(|e| format!("failed to cache attachment: {e}"))?;
    }
    Ok(AttachmentItem {
        id: format!("att-{}", &sha256[..16]),
        name,
        mime: mime_for_path(&source_path),
        byte_size: bytes.len() as u64,
        sha256,
        original_path: source_path.to_string_lossy().to_string(),
        cache_path: cache_path.to_string_lossy().to_string(),
        created_at: now_string(),
        source: "user-upload".to_owned(),
    })
}

fn write_text_file(args: &Value, work_context: &Value) -> Result<Value, String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing content".to_owned())?;
    let path = safe_output_path(args, work_context)?;
    let append = args.get("append").and_then(Value::as_bool).unwrap_or(false);
    if append {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("failed to open text file for append: {e}"))?;
        file.write_all(content.as_bytes())
            .map_err(|e| format!("failed to append text file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync appended text file: {e}"))?;
    } else {
        let temp_path = path.with_extension(format!(
            "{}.tmp",
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or("xiaoheng")
        ));
        fs::write(&temp_path, content)
            .map_err(|e| format!("failed to write temporary text file: {e}"))?;
        fs::rename(&temp_path, &path).map_err(|e| {
            let _ = fs::remove_file(&temp_path);
            format!("failed to commit text file atomically: {e}")
        })?;
    }
    let metadata = fs::metadata(&path).map_err(|e| format!("failed to verify text file: {e}"))?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "exists": path.exists(),
        "append": append,
        "mime": mime_for_path(&path),
        "actualByteSize": metadata.len(),
        "byteSize": content.as_bytes().len(),
        "charCount": content.chars().count(),
        "verificationNote": "write_text_file writes the complete content supplied by the model and verifies the target path. For overwrite writes it uses a temporary file plus rename. Use actualByteSize/exists and list_files/read_text_file metadata to verify, not truncated UI output."
    }))
}

fn read_text_file(args: &Value, work_context: &Value) -> Result<Value, String> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "missing path".to_owned())?;
    let path = if path.is_absolute() {
        path
    } else {
        active_workspace_dir(work_context)?.join(path)
    };
    let path = resolve_tool_path(path, work_context)?;
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(json!({
                "ok": true,
                "exists": false,
                "path": path.to_string_lossy(),
                "content": "",
                "warning": "file not found"
            }));
        }
        Err(error) => return Err(format!("failed to read file: {error}")),
    };
    let char_count = content.chars().count();
    let byte_size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let content_returned = first_chars(&content, READ_TEXT_FILE_CONTENT_LIMIT);
    let content_returned_chars = content_returned.chars().count();
    let content_truncated = char_count > content_returned_chars;
    Ok(json!({
        "ok": true,
        "exists": true,
        "path": path.to_string_lossy(),
        "byteSize": byte_size,
        "charCount": char_count,
        "contentReturnedChars": content_returned_chars,
        "contentTruncated": content_truncated,
        "contentPreview": first_chars(&content, TEXT_PREVIEW_CHARS),
        "contentTail": last_chars(&content, TEXT_PREVIEW_CHARS),
        "content": content_returned,
        "verificationNote": "content is a preview when contentTruncated=true. Do not conclude the file is empty or incomplete from truncated content; use byteSize, charCount, contentPreview and contentTail."
    }))
}

fn attachment_items_from_context(work_context: &Value) -> Vec<Value> {
    work_context
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn read_attachment_text(args: &Value, work_context: &Value) -> Result<Value, String> {
    let attachment_id = args
        .get("attachmentId")
        .or_else(|| args.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let requested_path = args
        .get("path")
        .or_else(|| args.get("cachePath"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if attachment_id.is_empty() && requested_path.is_empty() {
        return Err("missing attachmentId or path".to_owned());
    }
    let attachments = attachment_items_from_context(work_context);
    let item = attachments
        .iter()
        .find(|item| {
            item.get("id").and_then(Value::as_str) == Some(attachment_id)
                || (!requested_path.is_empty()
                    && item.get("cachePath").and_then(Value::as_str) == Some(requested_path))
        })
        .ok_or_else(|| "attachment not found in current conversation context".to_owned())?;
    let path = item
        .get("cachePath")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "attachment cachePath missing".to_owned())?;
    let bytes = fs::read(&path).map_err(|e| format!("failed to read attachment: {e}"))?;
    if bytes.contains(&0) {
        return Ok(json!({
            "ok": false,
            "attachmentId": item.get("id"),
            "name": item.get("name"),
            "mime": item.get("mime"),
            "byteSize": item.get("byteSize"),
            "cachePath": path.to_string_lossy(),
            "error": "attachment appears to be binary; use a suitable document/image parser or shell conversion tool instead of read_attachment_text"
        }));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        "attachment is not valid UTF-8 text; use a suitable parser or conversion tool".to_owned()
    })?;
    let char_count = content.chars().count();
    let content_returned = first_chars(&content, READ_TEXT_FILE_CONTENT_LIMIT);
    let content_returned_chars = content_returned.chars().count();
    Ok(json!({
        "ok": true,
        "attachmentId": item.get("id"),
        "name": item.get("name"),
        "mime": item.get("mime"),
        "byteSize": item.get("byteSize"),
        "cachePath": path.to_string_lossy(),
        "charCount": char_count,
        "contentReturnedChars": content_returned_chars,
        "contentTruncated": char_count > content_returned_chars,
        "contentPreview": first_chars(&content, TEXT_PREVIEW_CHARS),
        "contentTail": last_chars(&content, TEXT_PREVIEW_CHARS),
        "content": content_returned
    }))
}

fn list_files(args: &Value, work_context: &Value) -> Result<Value, String> {
    let dir = args
        .get("directory")
        .or_else(|| args.get("dir"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(active_workspace_dir(work_context)?);
    let dir = resolve_tool_path(dir, work_context)?;
    let read_dir = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(json!({
                "ok": true,
                "exists": false,
                "directory": dir.to_string_lossy(),
                "entries": [],
                "warning": "directory not found"
            }));
        }
        Err(error) => return Err(format!("failed to list directory: {error}")),
    };
    let all_entries = read_dir.filter_map(Result::ok).collect::<Vec<_>>();
    let total_count = all_entries.len();
    let mut extension_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in &all_entries {
        let path = entry.path();
        if path.is_file() {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !extension.is_empty() {
                *extension_counts.entry(extension).or_insert(0) += 1;
            }
        }
    }
    let entries = all_entries
        .iter()
        .take(200)
        .map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok();
            json!({
                "name": entry.file_name().to_string_lossy(),
                "path": path.to_string_lossy(),
                "isDir": path.is_dir(),
                "byteSize": metadata.as_ref().filter(|meta| meta.is_file()).map(|meta| meta.len())
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "exists": true,
        "directory": dir.to_string_lossy(),
        "count": total_count,
        "returnedCount": entries.len(),
        "entriesTruncated": total_count > entries.len(),
        "extensionCounts": extension_counts,
        "entries": entries,
        "verificationNote": "entries is a preview when entriesTruncated=true. Use count/returnedCount/extensionCounts for summary, and use shell_exec scripts to process complete large file sets instead of relying on the preview list."
    }))
}

fn decode_process_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn normalize_shell_success(
    command: &str,
    status_success: bool,
    stdout: &str,
    stderr: &str,
) -> bool {
    if status_success {
        return true;
    }
    let command_lower = command.to_lowercase();
    let combined = format!("{stdout}\n{stderr}").to_lowercase();
    command_lower.contains("pip install")
        && combined.contains("requirement already satisfied")
        && !combined.contains("error:")
        && !combined.contains("failed")
}

fn normalize_shell_command(command: &str, shell: &str) -> (String, Vec<String>) {
    let trimmed = command.trim();
    let lower = trimmed.to_lowercase();
    let mut notes = Vec::new();
    if lower.starts_with("pip install ") || lower == "pip install" {
        notes.push(
            "Normalized `pip install` to `python -m pip install` for stable Windows execution."
                .to_owned(),
        );
        let rest = trimmed
            .strip_prefix("pip")
            .or_else(|| trimmed.strip_prefix("PIP"))
            .unwrap_or(trimmed)
            .trim_start();
        return (format!("python -m pip {rest}"), notes);
    }
    if shell.eq_ignore_ascii_case("powershell")
        && (trimmed.contains(" | Select-Object ") || trimmed.contains("2>&1"))
    {
        notes.push("PowerShell native stderr redirection can turn successful native command stderr into NativeCommandError. WorkBuddy treats known benign pip notices with normalizeShellSuccess.".to_owned());
    }
    (command.to_owned(), notes)
}

fn remove_file_quiet(path: &Path) {
    let _ = fs::remove_file(path);
}

async fn shell_exec(args: &Value, work_context: &Value) -> Result<Value, String> {
    if setting_bool(work_context, "confirmShell", true) {
        return Err(permission_required_message("Shell 命令执行"));
    }
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "missing command".to_owned())?;
    let shell = args
        .get("shell")
        .and_then(Value::as_str)
        .unwrap_or("powershell");
    let shell = shell.to_owned();
    let timeout_seconds = args
        .get("timeoutSeconds")
        .or_else(|| args.get("timeout_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(120)
        .clamp(1, 900);
    let (command, command_notes) = normalize_shell_command(command, &shell);
    if shell.eq_ignore_ascii_case("powershell")
        && (command.contains(" || ") || command.contains(" && "))
    {
        let stderr = "PowerShell 5.1 does not support Bash/CMD-style '||' or '&&'. Use cmd shell for that command, or rewrite in PowerShell with `; if ($LASTEXITCODE -ne 0) { ... }`.";
        return Ok(json!({
            "ok": false,
            "status": 1,
            "stdout": "",
            "stdoutChars": 0,
            "stdoutTruncated": false,
            "stderr": stderr,
            "stderrChars": stderr.chars().count(),
            "stderrTruncated": false
        }));
    }
    let cwd = args
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or(active_workspace_dir(work_context)?);
    let cwd = resolve_tool_path(cwd, work_context)?;
    fs::create_dir_all(&cwd).map_err(|e| format!("failed to create cwd: {e}"))?;
    let command_for_status = command.clone();
    let shell_for_result = shell.clone();
    let cwd_for_result = cwd.clone();
    let started = SystemTime::now();
    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = if shell.eq_ignore_ascii_case("cmd") {
            let script_path = env::temp_dir().join(format!("xiaoheng-shell-{}.cmd", now_string()));
            let script = format!("@echo off\r\nchcp 65001 >NUL\r\n{command}\r\n");
            fs::write(&script_path, script)?;
            let mut cmd = Command::new("cmd");
            cmd.args(["/D", "/C", &script_path.to_string_lossy()]);
            hide_command_window(&mut cmd);
            let output = cmd.current_dir(&cwd).output();
            remove_file_quiet(&script_path);
            return output;
        } else {
            let mut cmd = Command::new("powershell.exe");
            let wrapped = format!(
                "[Console]::InputEncoding=[System.Text.Encoding]::UTF8; [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $OutputEncoding=[System.Text.Encoding]::UTF8; {command}"
            );
            cmd.args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &wrapped,
            ]);
            hide_command_window(&mut cmd);
            cmd
        };
        cmd.current_dir(cwd).output()
    });
    let output = tokio::time::timeout(Duration::from_secs(timeout_seconds), output)
        .await
        .map_err(|_| format!("shell command timed out after {timeout_seconds}s"))?
        .map_err(|e| format!("failed to join shell task: {e}"))?
        .map_err(|e| format!("failed to run shell command: {e}"))?;
    let duration_ms = started
        .elapsed()
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let stdout = decode_process_output(&output.stdout);
    let stderr = decode_process_output(&output.stderr);
    let ok = normalize_shell_success(
        &command_for_status,
        output.status.success(),
        &stdout,
        &stderr,
    );
    let stdout_chars = stdout.chars().count();
    let stderr_chars = stderr.chars().count();
    let stdout_returned = first_chars(&stdout, SHELL_OUTPUT_LIMIT);
    let stderr_returned = first_chars(&stderr, SHELL_OUTPUT_LIMIT);
    Ok(json!({
        "ok": ok,
        "status": output.status.code(),
        "shell": shell_for_result,
        "cwd": cwd_for_result.to_string_lossy(),
        "command": command_for_status,
        "timeoutSeconds": timeout_seconds,
        "durationMs": duration_ms,
        "notes": command_notes,
        "stdoutChars": stdout_chars,
        "stderrChars": stderr_chars,
        "stdoutReturnedChars": stdout_returned.chars().count(),
        "stderrReturnedChars": stderr_returned.chars().count(),
        "stdoutTruncated": stdout_chars > stdout_returned.chars().count(),
        "stderrTruncated": stderr_chars > stderr_returned.chars().count(),
        "stdoutPreview": first_chars(&stdout, TEXT_PREVIEW_CHARS),
        "stderrPreview": first_chars(&stderr, TEXT_PREVIEW_CHARS),
        "stdoutTail": last_chars(&stdout, TEXT_PREVIEW_CHARS),
        "stderrTail": last_chars(&stderr, TEXT_PREVIEW_CHARS),
        "stdout": stdout_returned,
        "stderr": stderr_returned,
        "verificationNote": "stdout/stderr may be previews when stdoutTruncated or stderrTruncated is true. Do not infer file contents or command side effects only from truncated shell output; verify with list_files or read_text_file metadata."
    }))
}

async fn run_powershell_script(script: String, cwd: PathBuf) -> Result<Value, String> {
    fs::create_dir_all(&cwd).map_err(|e| format!("failed to create cwd: {e}"))?;
    let output = tokio::task::spawn_blocking(move || {
        let wrapped = format!(
            "[Console]::InputEncoding=[System.Text.Encoding]::UTF8; [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $OutputEncoding=[System.Text.Encoding]::UTF8; {script}"
        );
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &wrapped,
            ])
            .current_dir(cwd);
        hide_command_window(&mut command);
        command.output()
    })
    .await
    .map_err(|e| format!("failed to join powershell task: {e}"))?
    .map_err(|e| format!("failed to run powershell script: {e}"))?;
    let stdout = decode_process_output(&output.stdout);
    let stderr = decode_process_output(&output.stderr);
    Ok(json!({
        "ok": output.status.success(),
        "status": output.status.code(),
        "stdout": stdout.chars().take(12000).collect::<String>(),
        "stderr": stderr.chars().take(12000).collect::<String>()
    }))
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[tauri::command]
fn start_heartbeat_scheduler(app: AppHandle) -> Result<Value, String> {
    let started = HEARTBEAT_SCHEDULER_STARTED.set(()).is_ok();
    if started {
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let epoch_seconds = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or_default();
                let _ = app_handle.emit(
                    "xiaoheng://heartbeat-tick",
                    json!({
                        "source": "tauri-backend",
                        "epochSeconds": epoch_seconds
                    }),
                );
            }
        });
    }
    Ok(json!({"ok": true, "started": started}))
}

async fn ensure_browser_debugger(
    client: &reqwest::Client,
    work_context: &Value,
    port: u16,
    initial_url: Option<&str>,
) -> Result<(), String> {
    let version_url = format!("http://127.0.0.1:{port}/json/version");
    if client.get(&version_url).send().await.is_ok() {
        return Ok(());
    }
    let cwd = active_workspace_dir(work_context)?;
    let profile_dir = cwd.join(".xiaoheng-browser-profile");
    fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("failed to create browser profile dir: {e}"))?;
    let url = initial_url.unwrap_or("about:blank");
    let port_arg = ps_quote(&format!("--remote-debugging-port={port}"));
    let profile_arg = ps_quote(&format!(
        "--user-data-dir={}",
        profile_dir.to_string_lossy()
    ));
    let url_arg = ps_quote(url);
    let script = format!(
        "$programFilesX86=${{env:ProgramFiles(x86)}}; $candidates=@((Join-Path $env:ProgramFiles 'Microsoft\\Edge\\Application\\msedge.exe'),(Join-Path $env:ProgramFiles 'Google\\Chrome\\Application\\chrome.exe'),(Join-Path $programFilesX86 'Microsoft\\Edge\\Application\\msedge.exe'),(Join-Path $programFilesX86 'Google\\Chrome\\Application\\chrome.exe')); $browser=$candidates|Where-Object{{$_ -and (Test-Path $_)}}|Select-Object -First 1; if(-not $browser){{throw 'Edge or Chrome was not found'}}; Start-Process -FilePath $browser -ArgumentList @({}, {}, '--no-first-run', '--new-window', {}); Start-Sleep -Milliseconds 900; Write-Output $browser",
        port_arg, profile_arg, url_arg
    );
    let result = run_powershell_script(script, cwd).await?;
    if result.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(())
    } else {
        Err(format!("failed to start browser debugger: {result}"))
    }
}

async fn browser_target_ws(
    client: &reqwest::Client,
    port: u16,
    url: Option<&str>,
    create_new: bool,
) -> Result<String, String> {
    if create_new {
        if let Some(target_url) = url {
            let endpoint = format!("http://127.0.0.1:{port}/json/new?{target_url}");
            let response = client
                .put(&endpoint)
                .send()
                .await
                .map_err(|e| format!("failed to create browser target: {e}"))?;
            if response.status().is_success() {
                let target: Value = response
                    .json()
                    .await
                    .map_err(|e| format!("failed to parse browser target: {e}"))?;
                if let Some(ws) = target.get("webSocketDebuggerUrl").and_then(Value::as_str) {
                    return Ok(ws.to_owned());
                }
            }
        }
    }
    let targets: Vec<Value> = client
        .get(format!("http://127.0.0.1:{port}/json"))
        .send()
        .await
        .map_err(|e| format!("failed to list browser targets: {e}"))?
        .json()
        .await
        .map_err(|e| format!("failed to parse browser targets: {e}"))?;
    targets
        .iter()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .and_then(|target| target.get("webSocketDebuggerUrl").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| "no debuggable browser page target found".to_owned())
}

async fn cdp_command(ws_url: &str, method: &str, params: Value) -> Result<Value, String> {
    let (mut socket, _) = connect_async(ws_url)
        .await
        .map_err(|e| format!("failed to connect browser websocket: {e}"))?;
    let command = json!({
        "id": 1,
        "method": method,
        "params": params
    });
    socket
        .send(Message::Text(command.to_string().into()))
        .await
        .map_err(|e| format!("failed to send browser command: {e}"))?;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|e| format!("failed to read browser command result: {e}"))?;
        let Message::Text(text) = message else {
            continue;
        };
        let payload: Value = serde_json::from_str(&text)
            .map_err(|e| format!("failed to parse browser command result: {e}; body={text}"))?;
        if payload.get("id").and_then(Value::as_i64) == Some(1) {
            if let Some(error) = payload.get("error") {
                return Err(format!("browser command {method} failed: {error}"));
            }
            return Ok(payload.get("result").cloned().unwrap_or_else(|| json!({})));
        }
    }
    Err(format!("browser command {method} returned no response"))
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

async fn browser_runtime_eval(ws_url: &str, expression: String) -> Result<Value, String> {
    let result = cdp_command(
        ws_url,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true
        }),
    )
    .await?;
    Ok(result
        .get("result")
        .and_then(|value| value.get("value"))
        .cloned()
        .unwrap_or(result))
}

async fn browser_control(
    client: &reqwest::Client,
    args: &Value,
    work_context: &Value,
) -> Result<Value, String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("open")
        .to_lowercase();
    let url = args
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let port = args
        .get("port")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0 && *value < 65536)
        .unwrap_or(9222) as u16;
    match action.as_str() {
        "open" | "navigate" => {
            let Some(url) = url else {
                return Err("browser_control open/navigate requires url".to_owned());
            };
            ensure_browser_debugger(client, work_context, port, Some(url)).await?;
            let ws = browser_target_ws(client, port, Some(url), true).await?;
            let result = cdp_command(&ws, "Page.navigate", json!({"url": url})).await?;
            Ok(json!({
                "ok": true,
                "action": action,
                "url": url,
                "debugPort": port,
                "detail": result
            }))
        }
        "fetch" | "extract" | "read" => {
            ensure_browser_debugger(client, work_context, port, url).await?;
            let ws = browser_target_ws(client, port, url, url.is_some()).await?;
            if let Some(url) = url {
                let _ = cdp_command(&ws, "Page.navigate", json!({"url": url})).await;
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
            let text = browser_runtime_eval(
                &ws,
                "document.body ? document.body.innerText : document.documentElement.innerText"
                    .to_owned(),
            )
            .await?;
            Ok(json!({"ok": true, "action": action, "url": url, "text": text, "debugPort": port}))
        }
        "screenshot" => {
            ensure_browser_debugger(client, work_context, port, url).await?;
            let ws = browser_target_ws(client, port, url, url.is_some()).await?;
            if let Some(url) = url {
                let _ = cdp_command(&ws, "Page.navigate", json!({"url": url})).await;
                tokio::time::sleep(Duration::from_millis(900)).await;
            }
            let result = cdp_command(
                &ws,
                "Page.captureScreenshot",
                json!({"format": "png", "captureBeyondViewport": true}),
            )
            .await?;
            let data = result
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| "browser screenshot returned no image data".to_owned())?;
            let filename = args
                .get("filename")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("browser-screenshot.png");
            let path = active_workspace_dir(work_context)?.join(safe_filename(filename));
            let bytes = BASE64_STANDARD
                .decode(data)
                .map_err(|e| format!("failed to decode browser screenshot: {e}"))?;
            fs::write(&path, bytes)
                .map_err(|e| format!("failed to write browser screenshot: {e}"))?;
            Ok(
                json!({"ok": true, "action": action, "path": path.to_string_lossy(), "debugPort": port}),
            )
        }
        "click" => {
            ensure_browser_debugger(client, work_context, port, url).await?;
            let ws = browser_target_ws(client, port, url, url.is_some()).await?;
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "browser_control click requires selector".to_owned())?;
            let expression = format!(
                r#"(async () => {{
const selector = {};
const el = document.querySelector(selector);
if (!el) return {{ ok: false, error: "selector not found", selector }};
el.scrollIntoView({{ block: "center", inline: "center" }});
const rect = el.getBoundingClientRect();
const x = rect.left + rect.width / 2;
const y = rect.top + rect.height / 2;
for (const type of ["mouseover", "mousedown", "mouseup", "click"]) {{
  el.dispatchEvent(new MouseEvent(type, {{ bubbles: true, cancelable: true, clientX: x, clientY: y, view: window }}));
}}
return {{ ok: true, selector, x, y, text: el.innerText || el.value || el.getAttribute("aria-label") || "" }};
}})()"#,
                js_string(selector)
            );
            let result = browser_runtime_eval(&ws, expression).await?;
            Ok(
                json!({"ok": result.get("ok").and_then(Value::as_bool).unwrap_or(false), "action": action, "result": result, "debugPort": port}),
            )
        }
        "type" | "fill" => {
            ensure_browser_debugger(client, work_context, port, url).await?;
            let ws = browser_target_ws(client, port, url, url.is_some()).await?;
            let selector = args
                .get("selector")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "browser_control type/fill requires selector".to_owned())?;
            let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
            let append = args.get("append").and_then(Value::as_bool).unwrap_or(false);
            let expression = format!(
                r#"(async () => {{
const selector = {};
const text = {};
const append = {};
const el = document.querySelector(selector);
if (!el) return {{ ok: false, error: "selector not found", selector }};
el.scrollIntoView({{ block: "center", inline: "center" }});
el.focus();
if ("value" in el) {{
  el.value = append ? String(el.value || "") + text : text;
}} else {{
  el.textContent = append ? String(el.textContent || "") + text : text;
}}
el.dispatchEvent(new InputEvent("input", {{ bubbles: true, inputType: "insertText", data: text }}));
el.dispatchEvent(new Event("change", {{ bubbles: true }}));
return {{ ok: true, selector, textLength: text.length }};
}})()"#,
                js_string(selector),
                js_string(text),
                if append { "true" } else { "false" }
            );
            let result = browser_runtime_eval(&ws, expression).await?;
            Ok(
                json!({"ok": result.get("ok").and_then(Value::as_bool).unwrap_or(false), "action": action, "result": result, "debugPort": port}),
            )
        }
        _ => Ok(json!({
            "ok": false,
            "action": action,
            "requires": "Edge or Chrome with DevTools remote debugging",
            "error": "Built-in browser_control supports open, navigate, fetch/read/extract, screenshot, click(selector), and type/fill(selector,text)."
        })),
    }
}

async fn computer_control(args: &Value, work_context: &Value) -> Result<Value, String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    let cwd = active_workspace_dir(work_context)?;
    match action.as_str() {
        "screenshot" => {
            let filename = args
                .get("filename")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("screenshot.png");
            let path = cwd.join(safe_filename(filename));
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; $bounds=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp=New-Object System.Drawing.Bitmap $bounds.Width,$bounds.Height; $graphics=[System.Drawing.Graphics]::FromImage($bmp); $graphics.CopyFromScreen($bounds.Location,[System.Drawing.Point]::Empty,$bounds.Size); $bmp.Save({}); $graphics.Dispose(); $bmp.Dispose(); Write-Output {}",
                ps_quote(&path.to_string_lossy()),
                ps_quote(&path.to_string_lossy())
            );
            let result = run_powershell_script(script, cwd).await?;
            Ok(
                json!({"ok": result.get("ok").and_then(Value::as_bool).unwrap_or(false), "path": path.to_string_lossy(), "detail": result}),
            )
        }
        "click" => {
            let x = args.get("x").and_then(Value::as_i64).unwrap_or(-1);
            let y = args.get("y").and_then(Value::as_i64).unwrap_or(-1);
            if x < 0 || y < 0 {
                return Err("computer_control click requires non-negative x and y".to_owned());
            }
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class Mouse {{ [DllImport(\"user32.dll\")] public static extern void mouse_event(int flags, int dx, int dy, int data, UIntPtr extra); }}'; [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({},{}); [Mouse]::mouse_event(0x0002,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [Mouse]::mouse_event(0x0004,0,0,0,[UIntPtr]::Zero);",
                x, y
            );
            run_powershell_script(script, cwd).await
        }
        "type" => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait({});",
                ps_quote(text)
            );
            run_powershell_script(script, cwd).await
        }
        "hotkey" => {
            let keys = args
                .get("keys")
                .or_else(|| args.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if keys.trim().is_empty() {
                return Err("computer_control hotkey requires keys".to_owned());
            }
            let send_keys = keys
                .replace("ctrl+", "^")
                .replace("control+", "^")
                .replace("shift+", "+")
                .replace("alt+", "%")
                .replace('+', "");
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait({});",
                ps_quote(&send_keys)
            );
            run_powershell_script(script, cwd).await
        }
        _ => Ok(json!({
            "ok": false,
            "action": action,
            "error": "Built-in computer_control supports screenshot, click, type, and hotkey. Window selection, visual localization, and element-level control require UI Automation or a vision runtime."
        })),
    }
}

fn skill_search(args: &Value, work_context: &Value) -> Value {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    let mut skills: Vec<Value> = work_context
        .get("enabledSkills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|skill| {
            let text = format!(
                "{} {} {} {:?}",
                skill
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                skill
                    .get("category")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                skill
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                skill.get("tools").unwrap_or(&Value::Null)
            )
            .to_lowercase();
            query.is_empty()
                || query.split_whitespace().any(|term| text.contains(term))
                || text.contains(&query)
        })
        .cloned()
        .collect();
    if prompt_matches_jimeng_image(&query)
        && !skills.iter().any(|skill| {
            skill.get("id").and_then(Value::as_str) == Some("jimeng-image")
                || skill.get("name").and_then(Value::as_str) == Some("jimeng-image")
        })
    {
        skills.insert(0, jimeng_image_skill_summary());
    }
    json!({ "ok": true, "skills": skills })
}

fn skill_read(args: &Value, work_context: &Value) -> Value {
    let skill_id = args
        .get("skillId")
        .or_else(|| args.get("skill_id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let skills = work_context
        .get("enabledSkills")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if skill_id.eq_ignore_ascii_case("jimeng-image")
        || skill_id.eq_ignore_ascii_case("jimeng")
        || skill_id == "即梦"
    {
        return json!({
            "ok": true,
            "skill": jimeng_image_skill_summary(),
            "content": jimeng_image_skill_content()
        });
    }
    let found = skills.iter().find(|skill| {
        skill["id"].as_str() == Some(skill_id)
            || skill["name"].as_str() == Some(skill_id)
            || skill["sourceId"].as_str() == Some(skill_id)
    });
    let Some(skill) = found else {
        return json!({"ok": false, "error": "skill not found"});
    };
    let Some(path) = skill["installPath"].as_str() else {
        return json!({"ok": true, "skill": skill, "content": skill["description"]});
    };
    let path = PathBuf::from(path);
    let skill_md = if path.is_dir() {
        find_skill_md(&path).unwrap_or_else(|| path.join("SKILL.md"))
    } else {
        path.clone()
    };
    let content = fs::read_to_string(&skill_md)
        .or_else(|_| fs::read_to_string(&path))
        .unwrap_or_else(|_| skill["description"].as_str().unwrap_or_default().to_owned());
    json!({
        "ok": true,
        "skill": skill,
        "content": content.chars().take(20000).collect::<String>()
    })
}

fn skill_list(work_context: &Value) -> Value {
    if let Some(installed) = work_context
        .get("installedSkills")
        .and_then(Value::as_array)
    {
        let enabled = installed
            .iter()
            .filter(|skill| {
                skill
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        let disabled = installed
            .iter()
            .filter(|skill| {
                !skill
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        return json!({
            "ok": true,
            "count": installed.len(),
            "installed": installed,
            "enabled": enabled,
            "disabled": disabled
        });
    }
    json!({
        "ok": true,
        "enabled": work_context.get("enabledSkills").cloned().unwrap_or_else(|| json!([])),
        "disabled": work_context.get("disabledSkills").cloned().unwrap_or_else(|| json!([]))
    })
}

fn memory_chunks(content: &str) -> Vec<Value> {
    content
        .split("\n## ")
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .enumerate()
        .map(|(index, chunk)| {
            let normalized = if index == 0 && chunk.starts_with("# ") {
                chunk.to_owned()
            } else if chunk.starts_with("## ") {
                chunk.to_owned()
            } else {
                format!("## {chunk}")
            };
            let title = normalized
                .lines()
                .next()
                .unwrap_or("Memory")
                .trim_start_matches('#')
                .trim()
                .to_owned();
            let hash = stable_hash(&normalized);
            json!({
                "id": format!("mem-{hash}"),
                "title": title,
                "hash": hash,
                "content": normalized,
                "charCount": normalized.chars().count()
            })
        })
        .collect()
}

fn keyword_score(query: &str, content: &str) -> f64 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return 0.1;
    }
    let content = content.to_lowercase();
    let mut score = 0.0;
    if content.contains(&query) {
        score += 3.0;
    }
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        for ch in query.chars().filter(|ch| !ch.is_whitespace()) {
            if content.contains(ch) {
                score += 0.2;
            }
        }
    } else {
        for term in terms {
            if content.contains(term) {
                score += 1.0;
            }
        }
    }
    score
}

fn vector_from_value(value: &Value) -> Option<Vec<f32>> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_f64().map(|value| value as f32))
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (left, right) in a.iter().zip(b.iter()) {
        let left = *left as f64;
        let right = *right as f64;
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

fn embedding_enabled(work_context: &Value) -> bool {
    work_context
        .get("settings")
        .and_then(|settings| settings.get("memoryEmbeddingEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn embedding_model(work_context: &Value) -> String {
    work_context
        .get("settings")
        .and_then(|settings| settings.get("memoryEmbeddingModel"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

async fn create_embedding(
    app: &AppHandle,
    client: &reqwest::Client,
    work_context: &Value,
    input: &str,
) -> Result<Vec<f32>, String> {
    let model = embedding_model(work_context);
    if model.is_empty() {
        return Err("memoryEmbeddingModel is empty".to_owned());
    }
    let provider = work_context
        .get("settings")
        .and_then(|settings| settings.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("compatible");
    let base_url = work_context
        .get("settings")
        .and_then(|settings| settings.get("baseUrl"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let base_url = provider_base_url(provider, base_url);
    let api_key = load_api_key(app, provider)?;
    let endpoint = format!("{}/embeddings", base_url.trim_end_matches('/'));
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&json!({
            "model": model,
            "input": input.chars().take(6000).collect::<String>()
        }))
        .send()
        .await
        .map_err(|e| format!("failed to call embedding model: {e}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read embedding response: {e}"))?;
    if !status.is_success() {
        return Err(format!(
            "embedding API returned {status}: {}",
            truncate_chars(&body, 800)
        ));
    }
    let value: Value = serde_json::from_str(&body).map_err(|e| {
        format!(
            "failed to parse embedding response: {e}; body={}",
            truncate_chars(&body, 800)
        )
    })?;
    let embedding = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("embedding"))
        .and_then(vector_from_value)
        .ok_or_else(|| "embedding response missing data[0].embedding".to_owned())?;
    Ok(embedding)
}

fn load_memory_index(app: &AppHandle) -> Value {
    memory_index_file(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({"version": 1, "chunks": []}))
}

async fn rebuild_memory_index(
    app: &AppHandle,
    client: &reqwest::Client,
    work_context: &Value,
) -> Result<Value, String> {
    let memory_path = memory_file(app)?;
    let index_path = memory_index_file(app)?;
    let content =
        fs::read_to_string(&memory_path).map_err(|e| format!("failed to read MEMORY.md: {e}"))?;
    let chunks = memory_chunks(&content);
    let existing = load_memory_index(app);
    let existing_chunks = existing
        .get("chunks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let model = embedding_model(work_context);
    let use_embedding = embedding_enabled(work_context) && !model.is_empty();
    let mut embedding_error: Option<String> = None;
    let mut final_chunks = Vec::new();
    for mut chunk in chunks {
        let hash = chunk
            .get("hash")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reusable = existing_chunks.iter().find(|item| {
            item.get("hash").and_then(Value::as_str) == Some(hash)
                && item.get("embeddingModel").and_then(Value::as_str) == Some(model.as_str())
                && item.get("embedding").and_then(Value::as_array).is_some()
        });
        if let Some(existing) = reusable {
            chunk["embedding"] = existing.get("embedding").cloned().unwrap_or(Value::Null);
            chunk["embeddingModel"] = json!(model.clone());
        }
        if use_embedding
            && embedding_error.is_none()
            && chunk.get("embedding").and_then(Value::as_array).is_none()
        {
            match create_embedding(
                app,
                client,
                work_context,
                chunk
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .await
            {
                Ok(vector) => {
                    chunk["embedding"] = json!(vector);
                    chunk["embeddingModel"] = json!(model.clone());
                }
                Err(error) => {
                    embedding_error = Some(error);
                }
            }
        }
        final_chunks.push(chunk);
    }
    let index = json!({
        "version": 1,
        "sourcePath": memory_path.to_string_lossy(),
        "embeddingEnabled": use_embedding,
        "embeddingModel": model.clone(),
        "embeddingError": embedding_error,
        "chunkCount": final_chunks.len(),
        "updatedAt": now_string(),
        "chunks": final_chunks
    });
    fs::write(
        index_path,
        serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("failed to write memory index: {e}"))?;
    Ok(index)
}

async fn memory_search_indexed(
    app: &AppHandle,
    client: &reqwest::Client,
    work_context: &Value,
    args: &Value,
) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(6)
        .min(20) as usize;
    let index = rebuild_memory_index(app, client, work_context).await?;
    let use_embedding =
        embedding_enabled(work_context) && !embedding_model(work_context).is_empty();
    let query_embedding = if use_embedding {
        create_embedding(app, client, work_context, &query)
            .await
            .ok()
    } else {
        None
    };
    let chunks = index
        .get("chunks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut scored = chunks
        .into_iter()
        .filter_map(|chunk| {
            let content = chunk.get("content").and_then(Value::as_str)?.to_owned();
            let keyword = keyword_score(&query, &content);
            let vector = chunk.get("embedding").and_then(vector_from_value);
            let semantic = query_embedding
                .as_ref()
                .zip(vector.as_ref())
                .map(|(query_vector, chunk_vector)| cosine_similarity(query_vector, chunk_vector))
                .unwrap_or(0.0);
            let score = keyword + semantic * 3.0;
            if score <= 0.0 {
                return None;
            }
            Some((score, keyword, semantic, chunk))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let results = scored
        .into_iter()
        .take(limit)
        .map(|(score, keyword, semantic, chunk)| {
            json!({
                "score": score,
                "keywordScore": keyword,
                "semanticScore": semantic,
                "title": chunk.get("title").cloned().unwrap_or(Value::Null),
                "content": chunk.get("content").and_then(Value::as_str).map(|value| truncate_chars(value, 1800)).unwrap_or_default()
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "path": memory_file(app)?.to_string_lossy(),
        "indexPath": memory_index_file(app)?.to_string_lossy(),
        "query": query,
        "resultCount": results.len(),
        "embeddingEnabled": use_embedding,
        "embeddingModel": embedding_model(work_context),
        "embeddingUsed": query_embedding.is_some(),
        "embeddingError": index.get("embeddingError").cloned().unwrap_or(Value::Null),
        "results": results
    }))
}

fn memory_read(app: &AppHandle) -> Result<Value, String> {
    let path = memory_file(app)?;
    let content =
        fs::read_to_string(&path).map_err(|e| format!("failed to read MEMORY.md: {e}"))?;
    let char_count = content.chars().count();
    let content_returned = first_chars(&content, 24000);
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "byteSize": fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0),
        "charCount": char_count,
        "contentReturnedChars": content_returned.chars().count(),
        "contentTruncated": char_count > content_returned.chars().count(),
        "contentPreview": first_chars(&content, TEXT_PREVIEW_CHARS),
        "contentTail": last_chars(&content, TEXT_PREVIEW_CHARS),
        "content": content_returned
    }))
}

fn memory_append(app: &AppHandle, args: &Value) -> Result<Value, String> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "memory_append requires content".to_owned())?;
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("记忆更新");
    let path = memory_file(app)?;
    let block = format!(
        "\n## {} · {}\n\n{}\n",
        chrono_like_today(),
        title.trim(),
        content.trim()
    );
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open MEMORY.md: {e}"))?;
    file.write_all(block.as_bytes())
        .map_err(|e| format!("failed to append MEMORY.md: {e}"))?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy(),
        "byteSize": block.as_bytes().len(),
        "charCount": block.chars().count()
    }))
}

fn team_roster(work_context: &Value) -> Value {
    json!({
        "ok": true,
        "coordinator": work_context.get("coordinator").cloned().unwrap_or(Value::Null),
        "roster": work_context.get("roster").cloned().unwrap_or_else(|| json!([])),
        "offices": work_context.get("offices").cloned().unwrap_or_else(|| json!([])),
        "tasks": work_context.get("tasks").cloned().unwrap_or_else(|| json!([]))
    })
}

fn task_plan_update(args: &Value, work_context: &Value) -> Value {
    let tasks = args
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let roster = work_context
        .get("roster")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let offices = work_context
        .get("offices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let default_office = offices
        .first()
        .and_then(|office| office.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("office-market-research");
    let default_owner = roster
        .iter()
        .find(|agent| agent.get("role").and_then(Value::as_str) == Some("xiaoheng"))
        .or_else(|| roster.first())
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("xiaoheng");
    let active_conversation_id = work_context
        .get("activeConversation")
        .and_then(|conversation| conversation.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let known_status = ["todo", "doing", "review", "done", "paused", "stopped"];
    let known_priority = ["low", "medium", "high"];
    let today = context_today(work_context);

    let updates: Vec<Value> = tasks
        .into_iter()
        .filter_map(|task| {
            let title = task.get("title").and_then(Value::as_str)?.trim();
            if title.is_empty() {
                return None;
            }
            let id = task
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("task-agent-{}-{}", now_string(), slug_id(title)));
            let status = task
                .get("status")
                .and_then(Value::as_str)
                .filter(|value| known_status.contains(value))
                .unwrap_or("todo");
            let priority = task
                .get("priority")
                .and_then(Value::as_str)
                .filter(|value| known_priority.contains(value))
                .unwrap_or("medium");
            let description = task.get("description").and_then(Value::as_str).unwrap_or(
                "Created and synchronized by Xiaoheng Agent from the conversation plan.",
            );
            let office_id = task
                .get("officeId")
                .or_else(|| task.get("office_id"))
                .and_then(Value::as_str)
                .unwrap_or(default_office);
            let owner_id = task
                .get("ownerId")
                .or_else(|| task.get("owner_id"))
                .and_then(Value::as_str)
                .unwrap_or(default_owner);
            let due_date = task
                .get("dueDate")
                .or_else(|| task.get("due_date"))
                .and_then(Value::as_str)
                .unwrap_or(&today);
            let upstream_ids = task
                .get("upstreamIds")
                .or_else(|| task.get("upstream_ids"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            let downstream_ids = task
                .get("downstreamIds")
                .or_else(|| task.get("downstream_ids"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            Some(json!({
                "id": id,
                "officeId": office_id,
                "title": title,
                "description": description,
                "ownerId": owner_id,
                "status": status,
                "priority": priority,
                "dueDate": due_date,
                "upstreamIds": upstream_ids,
                "downstreamIds": downstream_ids,
                "conversationId": active_conversation_id
            }))
        })
        .collect();

    json!({
        "ok": true,
        "tasks": updates,
        "note": "Task updates will be synchronized to the WorkBuddy task center."
    })
}

fn normalize_string_array(value: Option<&Value>) -> Value {
    value
        .and_then(Value::as_array)
        .map(|items| {
            Value::Array(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| json!(value.trim()))
                    .collect(),
            )
        })
        .unwrap_or_else(|| json!([]))
}

fn work_pool_update(args: &Value, work_context: &Value) -> Value {
    let roster = work_context
        .get("roster")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let default_manager = roster
        .iter()
        .find(|agent| agent.get("role").and_then(Value::as_str) == Some("manager"))
        .or_else(|| roster.first())
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("xiaoheng");
    let default_member = roster
        .iter()
        .find(|agent| agent.get("role").and_then(Value::as_str) == Some("har"))
        .or_else(|| roster.first())
        .and_then(|agent| agent.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("xiaoheng");

    let upsert_offices: Vec<Value> = args
        .get("upsertOffices")
        .or_else(|| args.get("upsert_offices"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|office| {
            let name = office.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let id = office
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("office-agent-{}-{}", now_string(), slug_id(name)));
            Some(json!({
                "id": id,
                "name": name,
                "description": office.get("description").and_then(Value::as_str).unwrap_or("Created or updated by Xiaoheng Agent."),
                "managerIds": normalize_string_array(office.get("managerIds").or_else(|| office.get("manager_ids"))).as_array().filter(|items| !items.is_empty()).cloned().unwrap_or_else(|| vec![json!(default_manager)]),
                "memberIds": normalize_string_array(office.get("memberIds").or_else(|| office.get("member_ids"))).as_array().filter(|items| !items.is_empty()).cloned().unwrap_or_else(|| vec![json!(default_member)]),
                "progress": office.get("progress").and_then(Value::as_u64).unwrap_or(0).min(100),
                "goals": normalize_string_array(office.get("goals")),
                "deliverables": normalize_string_array(office.get("deliverables")),
                "plan": normalize_string_array(office.get("plan"))
            }))
        })
        .collect();

    let delete_office_ids = normalize_string_array(
        args.get("deleteOfficeIds")
            .or_else(|| args.get("delete_office_ids")),
    );
    let delete_agent_ids = normalize_string_array(
        args.get("deleteAgentIds")
            .or_else(|| args.get("delete_agent_ids")),
    );
    let delete_task_ids = normalize_string_array(
        args.get("deleteTaskIds")
            .or_else(|| args.get("delete_task_ids")),
    );
    let upsert_agents: Vec<Value> = args
        .get("upsertAgents")
        .or_else(|| args.get("upsert_agents"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|agent| {
            let name = agent.get("name").and_then(Value::as_str)?.trim();
            if name.is_empty() {
                return None;
            }
            let id = agent
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("har-{}-{}", now_string(), slug_id(name)));
            let role = match agent.get("role").and_then(Value::as_str).unwrap_or("har") {
                "xiaoheng" => "xiaoheng",
                "manager" => "manager",
                _ => "har",
            };
            let status = match agent.get("status").and_then(Value::as_str).unwrap_or("idle") {
                "online" => "online",
                "working" => "working",
                _ => "idle",
            };
            Some(json!({
                "id": id,
                "name": name,
                "role": role,
                "title": agent.get("title").and_then(Value::as_str).unwrap_or("AI Specialist"),
                "specialty": agent.get("specialty").or_else(|| agent.get("description")).and_then(Value::as_str).unwrap_or("Created by Xiaoheng Agent."),
                "status": status,
                "color": agent.get("color").and_then(Value::as_str).unwrap_or("#2563eb"),
                "defaultModel": agent.get("defaultModel").or_else(|| agent.get("default_model")).cloned().unwrap_or(Value::Null),
                "sourceId": agent.get("sourceId").or_else(|| agent.get("source_id")).cloned().unwrap_or(Value::Null)
            }))
        })
        .collect();
    let task_updates = task_plan_update(&json!({"tasks": args.get("upsertTasks").or_else(|| args.get("upsert_tasks")).cloned().unwrap_or_else(|| json!([]))}), work_context)
        .get("tasks")
        .cloned()
        .unwrap_or_else(|| json!([]));

    let updates = json!({
        "upsertOffices": upsert_offices,
        "deleteOfficeIds": delete_office_ids,
        "upsertAgents": upsert_agents,
        "deleteAgentIds": delete_agent_ids,
        "upsertTasks": task_updates,
        "deleteTaskIds": delete_task_ids
    });
    json!({
        "ok": true,
        "workPoolUpdates": updates,
        "note": "Work pool updates will be applied to projects, people and tasks in the UI."
    })
}

fn date_from_unix_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as u32, d as u32)
}

fn chrono_like_today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| (value.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let (year, month, day) = date_from_unix_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn im_channel_status_tool() -> Value {
    json!({"ok": true, "channels": default_im_channels()})
}

#[allow(dead_code)]
fn agent_tools() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "skill_search",
                "description": "Search enabled WorkBuddy skills before planning or executing user work.",
                "parameters": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "skill_list",
                "description": "List enabled and disabled skills in the active work pool.",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "skill_read",
                "description": "Read the SKILL.md instructions for an enabled installed skill.",
                "parameters": {
                    "type": "object",
                    "properties": { "skillId": { "type": "string" } },
                    "required": ["skillId"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "skill_market_search",
                "description": "Search the real Harmony, ClawHub or SkillHub skill marketplace. Use this before installing marketplace skills; do not invent marketplace URLs.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "enum": ["harmony", "clawhub", "skillhub"] },
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
                        "offset": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["source", "query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "memory_search",
                "description": "Search long-term MEMORY.md without loading the entire memory file. Use this before answering questions that may depend on durable user preferences, project facts, decisions, or commitments.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "memory_read",
                "description": "Read the beginning of MEMORY.md when a full memory review is explicitly needed.",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "memory_append",
                "description": "Append durable, reusable long-term facts, preferences, decisions, or commitments to MEMORY.md.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "team_roster",
                "description": "Return Xiaoheng, managers, staff, offices and tasks from the active work pool.",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "task_plan_update",
                "description": "Create or update WorkBuddy task-center items from the agent's own plan, delegation, review, delivery, blocked, or stopped decisions. Use this for todo planning and status transitions.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string", "description": "Stable task id. Reuse it when updating the same task." },
                                    "officeId": { "type": "string" },
                                    "title": { "type": "string" },
                                    "description": { "type": "string" },
                                    "ownerId": { "type": "string", "description": "Agent/member id from team_roster." },
                                    "status": { "type": "string", "enum": ["todo", "doing", "review", "done", "paused", "stopped"] },
                                    "priority": { "type": "string", "enum": ["low", "medium", "high"] },
                                    "dueDate": { "type": "string", "description": "YYYY-MM-DD" },
                                    "upstreamIds": { "type": "array", "items": { "type": "string" } },
                                    "downstreamIds": { "type": "array", "items": { "type": "string" } }
                                },
                                "required": ["title", "description", "ownerId", "status", "priority"]
                            }
                        }
                    },
                    "required": ["tasks"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "work_pool_update",
                "description": "Create, update or delete WorkBuddy projects, people and tasks. Use this when the user asks to create/rename/edit/delete/dissolve project groups, modify team members, or manage the work pool. Deleting a project removes it from the UI and its related tasks.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "upsertOffices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "name": { "type": "string" },
                                    "description": { "type": "string" },
                                    "managerIds": { "type": "array", "items": { "type": "string" } },
                                    "memberIds": { "type": "array", "items": { "type": "string" } },
                                    "progress": { "type": "integer", "minimum": 0, "maximum": 100 },
                                    "goals": { "type": "array", "items": { "type": "string" } },
                                    "deliverables": { "type": "array", "items": { "type": "string" } },
                                    "plan": { "type": "array", "items": { "type": "string" } }
                                },
                                "required": ["name", "description"]
                            }
                        },
                        "deleteOfficeIds": { "type": "array", "items": { "type": "string" } },
                        "upsertAgents": { "type": "array", "items": { "type": "object" } },
                        "deleteAgentIds": { "type": "array", "items": { "type": "string" } },
                        "upsertTasks": { "type": "array", "items": { "type": "object" } },
                        "deleteTaskIds": { "type": "array", "items": { "type": "string" } }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "news_to_excel",
                "description": "Fetch headlines from a specified news website, generate a real .xlsx file, and save it to the active conversation workspace by default.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "sourceName": { "type": "string", "description": "Human readable source name, such as Tencent News or Baidu News" },
                        "url": { "type": "string", "description": "News homepage URL" },
                        "limit": { "type": "integer", "minimum": 3, "maximum": 50 },
                        "filename": { "type": "string", "description": "Excel filename ending with .xlsx" }
                    },
                    "required": ["sourceName", "url", "limit", "filename"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a public URL and return cleaned text.",
                "parameters": {
                    "type": "object",
                    "properties": { "url": { "type": "string" } },
                    "required": ["url"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_text_file",
                "description": "Write text content to a file. Prefer filename+directory, or pass path+content for an exact target path. Outside-workspace paths require current conversation permission mode Full Access.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "filename": { "type": "string" },
                        "content": { "type": "string" },
                        "path": { "type": "string", "description": "Optional exact target file path. If provided, it overrides filename and directory." },
                        "directory": { "type": "string", "description": "Optional output directory. Writing files or using outside-workspace paths requires current conversation permission mode Full Access." },
                        "append": { "type": "boolean", "description": "Append to the target file instead of atomically overwriting it." }
                    },
                    "required": ["content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_text_file",
                "description": "Read a text file from the active workspace. Outside-workspace paths require current conversation permission mode Full Access.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_attachment_text",
                "description": "Read UTF-8 text from a user-uploaded attachment already registered in the current conversation. Prefer attachmentId from the user message.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "attachmentId": { "type": "string" },
                        "path": { "type": "string", "description": "Optional cachePath from attachment metadata." }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_files",
                "description": "List files in the active workspace. Outside-workspace directories require current conversation permission mode Full Access.",
                "parameters": {
                    "type": "object",
                    "properties": { "directory": { "type": "string" } }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "shell_exec",
                "description": "Run a PowerShell or CMD command. Requires current conversation permission mode Full Access.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "shell": { "type": "string", "enum": ["powershell", "cmd"] },
                        "cwd": { "type": "string", "description": "Optional working directory. Default is active workspace." },
                        "timeoutSeconds": { "type": "integer", "minimum": 1, "maximum": 900, "description": "Optional timeout. Default 120 seconds." }
                    },
                    "required": ["command"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "browser_control",
                "description": "Control Edge/Chrome through a built-in DevTools bridge: open/navigate, read page text, capture PNG screenshot, click a CSS selector, or type/fill a CSS selector.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["open", "navigate", "fetch", "extract", "read", "screenshot", "click", "type", "fill"] },
                        "url": { "type": "string" },
                        "selector": { "type": "string" },
                        "text": { "type": "string" },
                        "filename": { "type": "string" },
                        "append": { "type": "boolean" },
                        "port": { "type": "integer", "description": "Optional DevTools port. Default 9222." }
                    },
                    "required": ["action"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "computer_control",
                "description": "Use built-in Windows automation for screenshot, coordinate click, text typing, or hotkeys. Element-level visual control requires a future UI Automation runtime.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["screenshot", "click", "type", "hotkey"] },
                        "target": { "type": "string" },
                        "text": { "type": "string" },
                        "keys": { "type": "string", "description": "SendKeys format or common text such as ctrl+s" },
                        "x": { "type": "integer" },
                        "y": { "type": "integer" },
                        "filename": { "type": "string" }
                    },
                    "required": ["action"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "im_channel_status",
                "description": "List configured IM channels and current connection status.",
                "parameters": { "type": "object", "properties": {} }
            }
        }
    ])
}

fn compact_agent_tools() -> Value {
    json!([
        {"type":"function","function":{"name":"skill_search","description":"Search enabled skills.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}}},
        {"type":"function","function":{"name":"skill_list","description":"List all installed skills available in the current conversation, including enabled and disabled status.","parameters":{"type":"object","properties":{}}}},
        {"type":"function","function":{"name":"skill_read","description":"Read installed skill instructions.","parameters":{"type":"object","properties":{"skillId":{"type":"string"}},"required":["skillId"]}}},
        {"type":"function","function":{"name":"team_roster","description":"Read people, projects and tasks.","parameters":{"type":"object","properties":{}}}},
        {"type":"function","function":{"name":"task_plan_update","description":"Sync todo/task status to task center.","parameters":{"type":"object","properties":{"tasks":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"officeId":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},"ownerId":{"type":"string"},"status":{"type":"string","enum":["todo","doing","review","done","paused","stopped"]},"priority":{"type":"string","enum":["low","medium","high"]},"dueDate":{"type":"string"},"upstreamIds":{"type":"array","items":{"type":"string"}},"downstreamIds":{"type":"array","items":{"type":"string"}}},"required":["title","description","ownerId","status","priority"]}}},"required":["tasks"]}}},
        {"type":"function","function":{"name":"work_pool_update","description":"Create/update/delete projects, people and tasks.","parameters":{"type":"object","properties":{"upsertOffices":{"type":"array","items":{"type":"object"}},"deleteOfficeIds":{"type":"array","items":{"type":"string"}},"upsertAgents":{"type":"array","items":{"type":"object"}},"deleteAgentIds":{"type":"array","items":{"type":"string"}},"upsertTasks":{"type":"array","items":{"type":"object"}},"deleteTaskIds":{"type":"array","items":{"type":"string"}}}}}},
        {"type":"function","function":{"name":"write_text_file","description":"Write file. Prefer filename+directory, or path+content for an exact target path. Outside-workspace paths require Full Access. Overwrite writes are atomic; set append=true only when extending an existing log/data file.","parameters":{"type":"object","properties":{"filename":{"type":"string"},"content":{"type":"string"},"directory":{"type":"string"},"path":{"type":"string"},"append":{"type":"boolean"}},"required":["content"]}}},
        {"type":"function","function":{"name":"read_text_file","description":"Read text file in workspace. Outside-workspace paths require Full Access.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}},
        {"type":"function","function":{"name":"read_attachment_text","description":"Read UTF-8 text from a user-uploaded attachment in current conversation by attachmentId.","parameters":{"type":"object","properties":{"attachmentId":{"type":"string"},"path":{"type":"string"}}}}},
        {"type":"function","function":{"name":"list_files","description":"List files in workspace. Outside-workspace paths require Full Access.","parameters":{"type":"object","properties":{"directory":{"type":"string"}}}}},
        {"type":"function","function":{"name":"shell_exec","description":"Run PowerShell or CMD. Requires current conversation permission mode Full Access. Prefer cmd for cmd/batch syntax and PowerShell for PowerShell syntax; use timeoutSeconds for long tasks.","parameters":{"type":"object","properties":{"command":{"type":"string"},"shell":{"type":"string","enum":["powershell","cmd"]},"cwd":{"type":"string"},"timeoutSeconds":{"type":"integer","minimum":1,"maximum":900}},"required":["command"]}}},
        {"type":"function","function":{"name":"browser_control","description":"Control Edge/Chrome via DevTools: open/navigate/read/screenshot/click selector/type selector.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["open","navigate","fetch","extract","read","screenshot","click","type","fill"]},"url":{"type":"string"},"selector":{"type":"string"},"text":{"type":"string"},"filename":{"type":"string"},"append":{"type":"boolean"},"port":{"type":"integer"}},"required":["action"]}}},
        {"type":"function","function":{"name":"web_fetch","description":"Fetch URL text.","parameters":{"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}}},
        {"type":"function","function":{"name":"news_to_excel","description":"Fetch news and save xlsx.","parameters":{"type":"object","properties":{"sourceName":{"type":"string"},"url":{"type":"string"},"limit":{"type":"integer","minimum":3,"maximum":50},"filename":{"type":"string"}},"required":["sourceName","url","limit","filename"]}}},
        {"type":"function","function":{"name":"skill_market_search","description":"Search skill markets.","parameters":{"type":"object","properties":{"source":{"type":"string","enum":["harmony","clawhub","skillhub"]},"query":{"type":"string"},"limit":{"type":"integer"},"offset":{"type":"integer"}},"required":["source","query"]}}},
        {"type":"function","function":{"name":"memory_search","description":"Search MEMORY.md.","parameters":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query"]}}},
        {"type":"function","function":{"name":"memory_read","description":"Read MEMORY.md metadata and preview.","parameters":{"type":"object","properties":{}}}},
        {"type":"function","function":{"name":"memory_append","description":"Append durable memory.","parameters":{"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["content"]}}},
        {"type":"function","function":{"name":"computer_control","description":"Screenshot/click/type/hotkey.","parameters":{"type":"object","properties":{"action":{"type":"string","enum":["screenshot","click","type","hotkey"]},"text":{"type":"string"},"keys":{"type":"string"},"x":{"type":"integer"},"y":{"type":"integer"},"filename":{"type":"string"}},"required":["action"]}}}
    ])
}

fn compact_tool_name(tool: &Value) -> Option<&str> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
}

fn filter_compact_tools(names: &BTreeSet<&'static str>) -> Value {
    let tools = compact_agent_tools()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|tool| {
            compact_tool_name(tool)
                .map(|name| names.contains(name))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    Value::Array(tools)
}

fn compact_agent_tools_for_prompt(prompt: &str) -> Option<Value> {
    let lower = prompt.to_lowercase();
    let mut names: BTreeSet<&'static str> = BTreeSet::new();
    let asks_file = lower.contains("file")
        || lower.contains("folder")
        || lower.contains("path")
        || lower.contains("save")
        || lower.contains("export")
        || lower.contains("read")
        || lower.contains("write")
        || lower.contains("markdown")
        || lower.contains("ppt")
        || lower.contains("pptx")
        || lower.contains("ppf")
        || lower.contains("excel")
        || lower.contains("xlsx")
        || lower.contains("word")
        || lower.contains("docx")
        || lower.contains("pdf")
        || prompt.contains("文件")
        || prompt.contains("目录")
        || prompt.contains("路径")
        || prompt.contains("保存")
        || prompt.contains("导出")
        || prompt.contains("读取")
        || prompt.contains("写入")
        || prompt.contains("生成");
    let asks_attachment = lower.contains("attachment")
        || prompt.contains("附件")
        || prompt.contains("上传")
        || prompt.contains("我发的文件")
        || prompt.contains("我上传");
    let asks_shell = lower.contains("shell")
        || lower.contains("cmd")
        || lower.contains("powershell")
        || lower.contains("python")
        || lower.contains("pip")
        || lower.contains("npm")
        || lower.contains("cargo")
        || lower.contains("build")
        || lower.contains("compile")
        || lower.contains("run ")
        || prompt.contains("执行")
        || prompt.contains("运行")
        || prompt.contains("安装")
        || prompt.contains("编译")
        || prompt.contains("打包");
    let asks_web = lower.contains("http")
        || lower.contains("web")
        || lower.contains("url")
        || lower.contains("search")
        || prompt.contains("网页")
        || prompt.contains("网址")
        || prompt.contains("搜索")
        || prompt.contains("网上")
        || prompt.contains("查找");
    let asks_dispatch = prompt_matches_dispatch_workflow(prompt);
    let asks_workspace = prompt.contains("项目")
        || prompt.contains("任务")
        || prompt.contains("人员")
        || prompt.contains("小组")
        || lower.contains("task")
        || lower.contains("project")
        || lower.contains("agent")
        || asks_dispatch;
    let asks_memory = prompt.contains("记忆") || lower.contains("memory");
    let asks_image_skill = prompt_matches_jimeng_image(prompt);
    let asks_skill = prompt.contains("技能") || lower.contains("skill") || asks_image_skill;
    let asks_desktop = prompt.contains("桌面")
        || prompt.contains("点击")
        || prompt.contains("截图")
        || lower.contains("desktop")
        || lower.contains("click")
        || lower.contains("screenshot");

    if asks_file {
        names.extend(["write_text_file", "read_text_file", "list_files"]);
    }
    if asks_attachment {
        names.insert("read_attachment_text");
    }
    if asks_shell || asks_file {
        names.insert("shell_exec");
    }
    if asks_web {
        names.insert("web_fetch");
        names.insert("browser_control");
    }
    if asks_workspace {
        names.extend(["team_roster", "task_plan_update", "work_pool_update"]);
    }
    if asks_dispatch {
        names.extend(["team_roster", "task_plan_update", "work_pool_update"]);
    }
    if asks_memory {
        names.extend(["memory_search", "memory_read", "memory_append"]);
    }
    if asks_skill {
        names.extend([
            "skill_list",
            "skill_search",
            "skill_read",
            "skill_market_search",
        ]);
    }
    if asks_image_skill {
        names.extend([
            "write_text_file",
            "read_text_file",
            "list_files",
            "shell_exec",
        ]);
    }
    if asks_desktop {
        names.insert("browser_control");
        names.insert("computer_control");
    }

    if names.is_empty() {
        None
    } else {
        Some(filter_compact_tools(&names))
    }
}

async fn execute_tool(
    app: &AppHandle,
    client: &reqwest::Client,
    work_context: &Value,
    name: &str,
    args: &Value,
) -> Value {
    let result = match name {
        "skill_search" => Ok(skill_search(args, work_context)),
        "skill_read" => Ok(skill_read(args, work_context)),
        "skill_list" => Ok(skill_list(work_context)),
        "memory_read" => memory_read(app),
        "memory_append" => {
            let result = memory_append(app, args);
            if result.is_ok() {
                let _ = rebuild_memory_index(app, client, work_context).await;
            }
            result
        }
        "memory_search" => memory_search_indexed(app, client, work_context, args).await,
        "skill_market_search" => {
            let source = args
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("harmony")
                .to_lowercase();
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(24)
                .min(50) as u32;
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as u32;
            search_skill_market_core(client, &source, &query, limit, offset).await
        }
        "team_roster" => Ok(team_roster(work_context)),
        "task_plan_update" => Ok(task_plan_update(args, work_context)),
        "work_pool_update" => Ok(work_pool_update(args, work_context)),
        "news_to_excel" => news_to_excel(client, args, work_context).await,
        "web_fetch" => web_fetch(client, args).await,
        "write_text_file" => write_text_file(args, work_context),
        "read_text_file" => read_text_file(args, work_context),
        "read_attachment_text" => read_attachment_text(args, work_context),
        "list_files" => list_files(args, work_context),
        "shell_exec" => shell_exec(args, work_context).await,
        "browser_control" => browser_control(client, args, work_context).await,
        "computer_control" => computer_control(args, work_context).await,
        "im_channel_status" => Ok(im_channel_status_tool()),
        other => Err(format!("unknown tool: {other}")),
    };
    match result {
        Ok(value) => value,
        Err(error) => json!({ "ok": false, "error": error }),
    }
}

async fn complete_openai_chat(
    app: &AppHandle,
    run_id: Option<&str>,
    client: &reqwest::Client,
    api_key: &str,
    settings: &AppSettings,
    messages: &[ChatMessage],
    work_context: &Value,
    system: &str,
) -> Result<String, String> {
    let base = if settings.provider == "ollama" && settings.base_url.trim().is_empty() {
        "http://127.0.0.1:11434/v1"
    } else {
        settings.base_url.trim_end_matches('/')
    };
    let endpoint = format!("{base}/chat/completions");
    let mut task_updates: Vec<Value> = Vec::new();
    let mut work_pool_updates = json!({});
    let mut run_todos: Vec<Value> = Vec::new();
    let mut tool_steps: Vec<Value> = Vec::new();
    let mut repeat_calls: BTreeMap<String, u32> = BTreeMap::new();
    let mut payload_messages = initial_openai_messages(system, messages);
    let latest_prompt = effective_user_prompt_from_messages(messages);
    let tool_catalog = compact_agent_tools_for_prompt(&latest_prompt);
    let mut forced_execution_corrections = 0u32;

    let use_tools = settings.provider != "ollama";
    let max_iterations = effective_agent_max_iterations(settings);
    let force_final_after = soft_final_iteration(max_iterations);
    for iteration in 0..max_iterations {
        let force_final = should_force_final(
            &latest_prompt,
            iteration,
            force_final_after,
            max_iterations,
            &tool_steps,
        );
        let mut next_messages = payload_messages.clone();
        if force_final {
            next_messages.push(final_summary_message(
                "Soft fence reached: enough tool work has been completed for this run.",
            ));
        }
        let request_messages = shrink_payload_messages(&next_messages, MAX_PAYLOAD_MESSAGE_CHARS);
        let mut payload = json!({"model": settings.model, "messages": request_messages, "temperature": 0.25, "max_tokens": settings.max_output_tokens});
        payload["stream"] = json!(false);
        if use_tools && !force_final {
            if let Some(tools) = tool_catalog.clone() {
                payload["tools"] = tools;
                payload["tool_choice"] = json!("auto");
            }
        }
        let (response, _payload_bytes) =
            send_openai_payload(client, &endpoint, api_key, settings, payload).await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("unknown")
            .to_owned();
        let raw_body = response
            .text()
            .await
            .map_err(|e| format!("failed to read model response body: {e}"))?;
        if !status.is_success() {
            return Err(format!(
                "model API returned {status} ({content_type}): {raw_body}"
            ));
        }
        let body: Value = serde_json::from_str(&raw_body).map_err(|e| {
            format!(
                "failed to parse model response as JSON ({content_type}): {e}; body={}",
                raw_body.chars().take(2000).collect::<String>()
            )
        })?;
        let Some(message) = body["choices"][0]["message"].as_object() else {
            return Err("model did not return a message".into());
        };
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut tool_calls = message.get("tool_calls").cloned();
        if tool_calls
            .as_ref()
            .and_then(Value::as_array)
            .map(|calls| calls.is_empty())
            .unwrap_or(true)
            && contains_pseudo_tool_call(&content)
        {
            let pseudo_calls = pseudo_tool_calls_from_text(&content);
            if pseudo_calls.is_empty() {
                return Err(pseudo_tool_call_parse_error());
            }
            tool_calls = Some(Value::Array(pseudo_calls));
        }
        let has_tool_calls = tool_calls
            .as_ref()
            .and_then(Value::as_array)
            .map(|calls| !calls.is_empty())
            .unwrap_or(false);
        if has_tool_calls && !content.trim().is_empty() {
            let note_content = remove_pseudo_tool_call_blocks(&content);
            if let Some(run_id) = run_id {
                if !note_content.trim().is_empty() {
                    emit_stream(
                        app,
                        run_id,
                        "assistant_note",
                        json!({"content": note_content.trim()}),
                    );
                } else if contains_pseudo_tool_call(&content) {
                    emit_stream(app, run_id, "content_reset", json!({"content": ""}));
                }
            }
        }
        if !has_tool_calls || force_final {
            let answer = if content.trim().is_empty() {
                if tool_steps.is_empty() {
                    "The model did not return any text.".to_owned()
                } else {
                    "已完成若干工具调用，但模型未生成最终文本。请查看执行记录中的文件路径、工具输出和任务状态。".to_owned()
                }
            } else {
                content
            };
            if !force_final
                && tool_steps.is_empty()
                && unverified_execution_block(
                    &latest_prompt,
                    &answer,
                    &session_permission_mode(work_context),
                )
            {
                if forced_execution_corrections < 2 {
                    forced_execution_corrections += 1;
                    payload_messages
                        .push(tool_execution_correction_message(&latest_prompt, &answer));
                    continue;
                }
                return Ok(blocked_unverified_execution_answer(&latest_prompt, &answer));
            }
            let answer = guard_completion_for_prompt(&latest_prompt, &answer, &tool_steps);
            return Ok(append_hidden_updates(
                &answer,
                &task_updates,
                &work_pool_updates,
                &run_todos,
                &tool_steps,
            ));
        }

        let compact_tool_calls = tool_calls.as_ref().map(compact_tool_calls_for_model);
        let assistant_payload_content = remove_pseudo_tool_call_blocks(&content);
        payload_messages.push(OpenAiMessage {
            role: "assistant".to_owned(),
            content: if assistant_payload_content.is_empty() {
                None
            } else {
                Some(compact_chat_content(&assistant_payload_content))
            },
            tool_call_id: None,
            tool_calls: compact_tool_calls,
        });

        for call in tool_calls
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
        {
            let id = call["id"].as_str().unwrap_or("").to_owned();
            let name = call["function"]["name"].as_str().unwrap_or("");
            let args_text = call["function"]["arguments"].as_str().unwrap_or("{}");
            let args = parse_tool_arguments(args_text);
            if let Some(run_id) = run_id {
                let start_step = json!({
                    "id": id,
                    "name": name,
                    "input": args,
                    "completed": false
                });
                emit_stream(app, run_id, "tool_start", json!({"step": start_step}));
            }
            let fingerprint = tool_call_fingerprint(name, &args);
            let repeat_count = repeat_calls
                .entry(fingerprint)
                .and_modify(|count| *count += 1)
                .or_insert(1);
            let result = if *repeat_count > repeat_limit_for_tool(name) {
                json!({
                    "ok": false,
                    "error": "Repeated identical tool call suppressed by WorkBuddy soft fence. Stop repeating this tool call; summarize current progress or choose a different next action."
                })
            } else {
                execute_tool(app, client, work_context, name, &args).await
            };
            if mutates_tool_observable_state(name, &result) {
                repeat_calls.clear();
            }
            if name == "task_plan_update" {
                if let Some(tasks) = result.get("tasks").and_then(Value::as_array) {
                    task_updates.extend(tasks.iter().cloned());
                    run_todos = make_todos_from_tasks(tasks);
                    if let Some(run_id) = run_id {
                        emit_stream(app, run_id, "todo", json!({"todos": run_todos}));
                    }
                }
            }
            if name == "work_pool_update" {
                if let Some(updates) = result.get("workPoolUpdates") {
                    work_pool_updates = updates.clone();
                    if let Some(tasks) = updates.get("upsertTasks").and_then(Value::as_array) {
                        task_updates.extend(tasks.iter().cloned());
                        run_todos = make_todos_from_tasks(tasks);
                        if let Some(run_id) = run_id {
                            emit_stream(app, run_id, "todo", json!({"todos": run_todos}));
                        }
                    }
                }
            }
            let step =
                compact_tool_step_for_ui(&id, name, &args, &result, MAX_TOOL_RESULT_CHARS_FOR_UI);
            tool_steps.push(step.clone());
            if let Some(run_id) = run_id {
                emit_stream(
                    app,
                    run_id,
                    "tool_end",
                    json!({
                        "step": step,
                        "tasks": task_updates,
                        "workPoolUpdates": work_pool_updates,
                        "todos": run_todos
                    }),
                );
            }
            payload_messages.push(OpenAiMessage {
                role: "tool".to_owned(),
                content: Some(compact_tool_result_for_model(
                    name,
                    &result,
                    MAX_TOOL_RESULT_CHARS_FOR_MODEL,
                )),
                tool_call_id: Some(id),
                tool_calls: None,
            });
        }
    }
    Ok(append_hidden_updates(
        "已达到本轮 Agent 最大执行轮次。已保留完成的工具调用、任务同步和中间结果；请查看执行记录，或继续追问让我基于现有结果收尾。",
        &task_updates,
        &work_pool_updates,
        &run_todos,
        &tool_steps,
    ))
}

struct StreamChatEnv<'a> {
    app: AppHandle,
    run_id: String,
    client: &'a reqwest::Client,
    api_key: &'a str,
    settings: &'a AppSettings,
    work_context: &'a Value,
    system: &'a str,
}

async fn complete_openai_chat_stream(
    env: StreamChatEnv<'_>,
    messages: &[ChatMessage],
) -> Result<(), String> {
    let StreamChatEnv {
        app,
        run_id,
        client,
        api_key,
        settings,
        work_context,
        system,
    } = env;
    let base = if settings.provider == "ollama" && settings.base_url.trim().is_empty() {
        "http://127.0.0.1:11434/v1"
    } else {
        settings.base_url.trim_end_matches('/')
    };
    let endpoint = format!("{base}/chat/completions");
    let mut task_updates: Vec<Value> = Vec::new();
    let mut work_pool_updates = json!({});
    let mut run_todos: Vec<Value> = Vec::new();
    let mut tool_steps: Vec<Value> = Vec::new();
    let mut final_content = String::new();
    let mut payload_messages = initial_openai_messages(system, messages);
    let mut repeat_calls: BTreeMap<String, u32> = BTreeMap::new();
    let latest_prompt = effective_user_prompt_from_messages(messages);
    let tool_catalog = compact_agent_tools_for_prompt(&latest_prompt);
    let mut forced_execution_corrections = 0u32;
    emit_stream(
        &app,
        &run_id,
        "started",
        json!({"message": "小亨 Gateway 已连接模型，正在按需处理。"}),
    );

    let use_tools = settings.provider != "ollama";
    let max_iterations = effective_agent_max_iterations(settings);
    let force_final_after = soft_final_iteration(max_iterations);
    for iteration in 0..max_iterations {
        if run_stop_requested(&run_id) {
            emit_stopped(&app, &run_id, &task_updates, &run_todos, &tool_steps);
            clear_run_stop(&run_id);
            return Ok(());
        }
        let force_final = should_force_final(
            &latest_prompt,
            iteration,
            force_final_after,
            max_iterations,
            &tool_steps,
        );
        let mut next_messages = payload_messages.clone();
        if force_final {
            next_messages.push(final_summary_message(
                "Soft fence reached: enough tool work has been completed for this run.",
            ));
        }
        let request_messages = shrink_payload_messages(&next_messages, MAX_PAYLOAD_MESSAGE_CHARS);
        let mut payload = json!({
            "model": settings.model,
            "messages": request_messages,
            "temperature": 0.25,
            "max_tokens": settings.max_output_tokens,
            "stream": true
        });
        if use_tools && !force_final {
            if let Some(tools) = tool_catalog.clone() {
                payload["tools"] = tools;
                payload["tool_choice"] = json!("auto");
            }
        }
        let payload_bytes = serde_json::to_vec(&payload)
            .map(|value| value.len())
            .unwrap_or_default();
        let response = match client
            .post(&endpoint)
            .bearer_auth(api_key)
            .header("accept", "text/event-stream, application/json")
            .header("accept-encoding", "identity")
            .header("cache-control", "no-cache")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(first_error) if payload.get("tools").is_some() => {
                let mut fallback_payload = payload.clone();
                if let Some(object) = fallback_payload.as_object_mut() {
                    object.remove("tools");
                    object.remove("tool_choice");
                    object.insert("stream".to_owned(), json!(false));
                }
                match send_openai_payload(client, &endpoint, api_key, settings, fallback_payload)
                    .await
                {
                    Ok((fallback_response, fallback_payload_bytes)) => {
                        let status = fallback_response.status();
                        let body = fallback_response.text().await.unwrap_or_default();
                        if !status.is_success() {
                            return Err(format!("stream request failed before response (payload={} bytes): {first_error}; no-tools retry returned {status}: {body}", payload_bytes));
                        }
                        let parsed: Value = serde_json::from_str(&body)
                            .map_err(|e| format!("stream request failed before response (payload={} bytes): {first_error}; no-tools retry payload={} bytes returned non-json: {e}; body={}", payload_bytes, fallback_payload_bytes, body.chars().take(2000).collect::<String>()))?;
                        let content = parsed["choices"][0]["message"]["content"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        if contains_pseudo_tool_call(&content) {
                            emit_stream(
                                &app,
                                &run_id,
                                "error",
                                json!({
                                    "error": format!("stream request failed before response (payload={} bytes): {first_error}; no-tools retry returned pseudo tool_call text instead of a final answer. {}", payload_bytes, pseudo_tool_call_parse_error()),
                                    "tasks": task_updates,
                                    "workPoolUpdates": work_pool_updates,
                                    "todos": run_todos,
                                    "toolSteps": tool_steps
                                }),
                            );
                            return Ok(());
                        }
                        if prompt_requires_real_tool_execution(&latest_prompt) {
                            emit_stream(
                                &app,
                                &run_id,
                                "error",
                                json!({
                                    "error": format!("stream request with tools failed before response (payload={} bytes): {first_error}; no-tools retry returned text but this request requires real tool execution, so WorkBuddy rejected it as incomplete. Retry with a stable tool-call-capable model or disable streaming.", payload_bytes),
                                    "tasks": task_updates,
                                    "workPoolUpdates": work_pool_updates,
                                    "todos": run_todos,
                                    "toolSteps": tool_steps
                                }),
                            );
                            return Ok(());
                        }
                        emit_stream(
                            &app,
                            &run_id,
                            "done",
                            json!({
                                "content": if content.trim().is_empty() { "The model returned no text after no-tools retry." } else { content.as_str() },
                                "tasks": task_updates,
                                "workPoolUpdates": work_pool_updates,
                                "todos": run_todos,
                                "toolSteps": tool_steps
                            }),
                        );
                        return Ok(());
                    }
                    Err(retry_error) => {
                        return Err(format!("stream request failed before response (payload={} bytes): {first_error}; no-tools retry failed: {retry_error}", payload_bytes));
                    }
                }
            }
            Err(error) => {
                return Err(format!("failed to connect model service (provider={}, model={}, url={}, payload={} bytes): {error}", settings.provider, settings.model, endpoint, payload_bytes));
            }
        };
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "failed to read error body".to_owned());
            if body.contains("Model does not exist") || body.contains("\"code\":20012") {
                return Err(format!(
                    "Current model `{}` does not exist on provider `{}`. Verify and pull the model list in Settings, then choose a model returned by this provider, or switch to the matching provider and retry. Raw response: {body}",
                    settings.model,
                    settings.provider
                ));
            }
            return Err(format!("model API returned {status}: {body}"));
        }
        let mut assistant_content = String::new();
        let mut tool_call_map: BTreeMap<usize, Value> = BTreeMap::new();
        let mut buffer = String::new();
        let mut pseudo_content_mode = false;
        let mut byte_stream = response.bytes_stream();
        while let Some(chunk) = byte_stream.next().await {
            if run_stop_requested(&run_id) {
                emit_stopped(&app, &run_id, &task_updates, &run_todos, &tool_steps);
                clear_run_stop(&run_id);
                return Ok(());
            }
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let message = format!(
                        "Model streaming response interrupted or failed to decode: {error}. Falling back to one non-streaming retry to check whether this is a provider SSE or gateway issue."
                    );
                    emit_stream(
                        &app,
                        &run_id,
                        "token",
                        json!({"content": format!("\n\n{message}\n\n")}),
                    );
                    let fallback_messages = payload_messages
                        .iter()
                        .filter_map(|message| {
                            message.content.as_ref().map(|content| ChatMessage {
                                role: message.role.clone(),
                                content: content.clone(),
                                attachments: Vec::new(),
                            })
                        })
                        .collect::<Vec<_>>();
                    match complete_openai_chat(
                        &app,
                        Some(&run_id),
                        client,
                        api_key,
                        settings,
                        &fallback_messages,
                        work_context,
                        system,
                    )
                    .await
                    {
                        Ok(answer) => {
                            emit_stream(
                                &app,
                                &run_id,
                                "done",
                                json!({
                                    "content": strip_hidden_agent_markers(&answer),
                                    "tasks": task_updates,
                                    "workPoolUpdates": work_pool_updates,
                                    "todos": run_todos,
                                    "toolSteps": tool_steps
                                }),
                            );
                            return Ok(());
                        }
                        Err(fallback_error) => {
                            let error_message = format!(
                                "{message}\n\nNon-streaming retry also failed: {fallback_error}. Check provider, proxy/network, model ID, or disable token streaming and retry."
                            );
                            emit_stream(
                                &app,
                                &run_id,
                                "error",
                                json!({
                                    "error": if final_content.trim().is_empty() { error_message.clone() } else { format!("{}\n\n{}", final_content.trim(), error_message) },
                                    "tasks": task_updates,
                                    "workPoolUpdates": work_pool_updates,
                                    "todos": run_todos,
                                    "toolSteps": tool_steps
                                }),
                            );
                            return Ok(());
                        }
                    }
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_owned();
                buffer = buffer[pos + 1..].to_owned();
                if !line.starts_with("data:") {
                    continue;
                }
                let data = line.trim_start_matches("data:").trim();
                if data == "[DONE]" || data.is_empty() {
                    continue;
                }
                let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                    continue;
                };
                let delta = &chunk["choices"][0]["delta"];
                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    assistant_content.push_str(content);
                    final_content.push_str(content);
                    if contains_pseudo_tool_call(&assistant_content) {
                        pseudo_content_mode = true;
                        emit_stream(
                            &app,
                            &run_id,
                            "content_reset",
                            json!({"content": remove_pseudo_tool_call_blocks(&assistant_content)}),
                        );
                    } else if !pseudo_content_mode {
                        emit_stream(&app, &run_id, "token", json!({"content": content}));
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tool_call_map.entry(index).or_insert_with(|| {
                            json!({
                                "id": "",
                                "type": "function",
                                "function": { "name": "", "arguments": "" }
                            })
                        });
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            entry["id"] = json!(id);
                        }
                        if let Some(name) = call["function"].get("name").and_then(Value::as_str) {
                            let old = entry["function"]["name"].as_str().unwrap_or_default();
                            entry["function"]["name"] = json!(format!("{old}{name}"));
                        }
                        if let Some(args) =
                            call["function"].get("arguments").and_then(Value::as_str)
                        {
                            let old = entry["function"]["arguments"].as_str().unwrap_or_default();
                            entry["function"]["arguments"] = json!(format!("{old}{args}"));
                        }
                    }
                }
            }
        }

        let mut tool_calls: Vec<Value> = tool_call_map.into_values().collect();
        if tool_calls.is_empty() && contains_pseudo_tool_call(&assistant_content) {
            tool_calls = pseudo_tool_calls_from_text(&assistant_content);
            if tool_calls.is_empty() {
                emit_stream(&app, &run_id, "content_reset", json!({"content": ""}));
                emit_stream(
                    &app,
                    &run_id,
                    "error",
                    json!({
                        "error": pseudo_tool_call_parse_error(),
                        "tasks": task_updates,
                        "workPoolUpdates": work_pool_updates,
                        "todos": run_todos,
                        "toolSteps": tool_steps
                    }),
                );
                return Ok(());
            }
            assistant_content = remove_pseudo_tool_call_blocks(&assistant_content);
            final_content = remove_pseudo_tool_call_blocks(&final_content);
            emit_stream(
                &app,
                &run_id,
                "content_reset",
                json!({"content": final_content}),
            );
        }
        if tool_calls.is_empty() || force_final {
            let mut final_content =
                guard_completion_for_prompt(&latest_prompt, &final_content, &tool_steps);
            if !force_final
                && tool_steps.is_empty()
                && unverified_execution_block(
                    &latest_prompt,
                    &final_content,
                    &session_permission_mode(work_context),
                )
            {
                if forced_execution_corrections < 2 {
                    forced_execution_corrections += 1;
                    emit_stream(&app, &run_id, "content_reset", json!({"content": ""}));
                    emit_stream(
                        &app,
                        &run_id,
                        "assistant_note",
                        json!({"content": "Gateway 检测到模型只输出说明但没有执行工具，正在自动纠偏并要求继续真实执行。"}),
                    );
                    payload_messages.push(tool_execution_correction_message(
                        &latest_prompt,
                        &final_content,
                    ));
                    final_content.clear();
                    continue;
                }
                emit_stream(&app, &run_id, "content_reset", json!({"content": ""}));
                emit_stream(
                    &app,
                    &run_id,
                    "done",
                    json!({
                        "content": blocked_unverified_execution_answer(&latest_prompt, &final_content),
                        "tasks": task_updates,
                        "workPoolUpdates": work_pool_updates,
                        "todos": run_todos,
                        "toolSteps": tool_steps
                    }),
                );
                return Ok(());
            }
            emit_stream(
                &app,
                &run_id,
                "done",
                json!({
                    "content": final_content,
                    "tasks": task_updates,
                    "workPoolUpdates": work_pool_updates,
                    "todos": run_todos,
                    "toolSteps": tool_steps
                }),
            );
            return Ok(());
        }

        let compact_tool_calls =
            Value::Array(tool_calls.iter().map(compact_tool_call_for_model).collect());
        payload_messages.push(OpenAiMessage {
            role: "assistant".to_owned(),
            content: if assistant_content.trim().is_empty() {
                None
            } else {
                Some(compact_chat_content(&assistant_content))
            },
            tool_call_id: None,
            tool_calls: Some(compact_tool_calls),
        });

        for call in tool_calls {
            if run_stop_requested(&run_id) {
                emit_stopped(&app, &run_id, &task_updates, &run_todos, &tool_steps);
                clear_run_stop(&run_id);
                return Ok(());
            }
            let id = call["id"].as_str().unwrap_or("").to_owned();
            let name = call["function"]["name"].as_str().unwrap_or("");
            let args_text = call["function"]["arguments"].as_str().unwrap_or("{}");
            let args = parse_tool_arguments(args_text);
            let start_step = json!({
                "id": id,
                "name": name,
                "input": args,
                "completed": false
            });
            emit_stream(&app, &run_id, "tool_start", json!({"step": start_step}));
            let fingerprint = tool_call_fingerprint(name, &args);
            let repeat_count = repeat_calls
                .entry(fingerprint)
                .and_modify(|count| *count += 1)
                .or_insert(1);
            let result = if *repeat_count > repeat_limit_for_tool(name) {
                json!({
                    "ok": false,
                    "error": "Repeated identical tool call suppressed by WorkBuddy soft fence. Stop repeating this tool call; summarize current progress or choose a different next action."
                })
            } else {
                execute_tool(&app, client, work_context, name, &args).await
            };
            if mutates_tool_observable_state(name, &result) {
                repeat_calls.clear();
            }
            if run_stop_requested(&run_id) {
                emit_stopped(&app, &run_id, &task_updates, &run_todos, &tool_steps);
                clear_run_stop(&run_id);
                return Ok(());
            }
            if name == "task_plan_update" {
                if let Some(tasks) = result.get("tasks").and_then(Value::as_array) {
                    task_updates.extend(tasks.iter().cloned());
                    run_todos = make_todos_from_tasks(tasks);
                    emit_stream(&app, &run_id, "todo", json!({"todos": run_todos}));
                }
            }
            if name == "work_pool_update" {
                if let Some(updates) = result.get("workPoolUpdates") {
                    work_pool_updates = updates.clone();
                    if let Some(tasks) = updates.get("upsertTasks").and_then(Value::as_array) {
                        task_updates.extend(tasks.iter().cloned());
                        run_todos = make_todos_from_tasks(tasks);
                        emit_stream(&app, &run_id, "todo", json!({"todos": run_todos}));
                    }
                }
            }
            let step = json!({
                "id": id,
                "name": name,
                "input": args,
                "result": truncate_chars(&result.to_string(), (settings.tool_result_token_limit as usize).max(MAX_TOOL_RESULT_CHARS_FOR_UI).min(12000)),
                "isError": result.get("ok").and_then(Value::as_bool) == Some(false),
                "completed": true
            });
            tool_steps.push(step.clone());
            emit_stream(
                &app,
                &run_id,
                "tool_end",
                json!({"step": step, "tasks": task_updates, "workPoolUpdates": work_pool_updates, "todos": run_todos}),
            );
            payload_messages.push(OpenAiMessage {
                role: "tool".to_owned(),
                content: Some(compact_tool_result_for_model(
                    name,
                    &result,
                    MAX_TOOL_RESULT_CHARS_FOR_MODEL,
                )),
                tool_call_id: Some(id),
                tool_calls: None,
            });
        }
    }
    let warning = format!(
        "{}\n\n已达到本轮 Agent 最大执行轮次（{}）。已保留完成的工具调用、任务同步和中间结果；请查看执行记录，或继续追问让我基于现有结果收尾。",
        final_content.trim(),
        max_iterations
    )
    .trim()
    .to_owned();
    emit_stream(
        &app,
        &run_id,
        "done",
        json!({
            "content": warning,
            "tasks": task_updates,
            "workPoolUpdates": work_pool_updates,
            "todos": run_todos,
            "toolSteps": tool_steps
        }),
    );
    Ok(())
}

#[tauri::command]
async fn complete_chat(
    app: AppHandle,
    messages: Vec<ChatMessage>,
    settings: AppSettings,
    work_context: Value,
) -> Result<String, String> {
    observe_security_settings(&settings);
    let latest_prompt = latest_user_prompt_from_messages(&messages);
    if let Some(reply) = simple_gateway_reply(&latest_prompt) {
        return Ok(reply);
    }
    if let Some(reply) = current_time_reply(&work_context, &latest_prompt) {
        return Ok(reply);
    }
    if let Some(reply) = installed_skill_list_reply(&work_context, &latest_prompt) {
        return Ok(reply);
    }
    let api_key = load_api_key(&app, &settings.provider)?;
    let client = model_client_builder(settings.llm_timeout_seconds)
        .build()
        .map_err(|e| e.to_string())?;
    let system = summarize_work_context(&work_context);

    if settings.provider == "anthropic" {
        let endpoint = format!("{}/messages", settings.base_url.trim_end_matches('/'));
        let items: Vec<Value> = messages
            .iter()
            .map(|message| json!({"role": message.role, "content": message.content}))
            .collect();
        let response = client
            .post(endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({"model": settings.model, "max_tokens": settings.max_output_tokens, "system": system, "messages": items}))
            .send()
            .await
            .map_err(|e| format!("failed to connect Anthropic: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("failed to parse model response: {e}"))?;
        if !status.is_success() {
            return Err(format!("model API returned {status}: {body}"));
        }
        return body["content"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "model did not return text".into());
    }

    complete_openai_chat(
        &app,
        None,
        &client,
        &api_key,
        &settings,
        &messages,
        &work_context,
        &system,
    )
    .await
}

#[tauri::command]
fn stop_chat_stream(run_id: String) -> Result<(), String> {
    STOPPED_RUNS
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
        .map_err(|e| e.to_string())?
        .insert(run_id);
    Ok(())
}

#[tauri::command]
fn archive_task_tree(app: AppHandle, task: Value) -> Result<(), String> {
    let archived_at = task
        .get("archivedAt")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let day = archived_at.get(0..10).unwrap_or("unknown");
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("task")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("archive-tasks")
        .join(day);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec_pretty(&task).map_err(|e| e.to_string())?;
    fs::write(base.join(format!("{task_id}.json")), body).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn complete_chat_stream(
    app: AppHandle,
    run_id: String,
    messages: Vec<ChatMessage>,
    settings: AppSettings,
    work_context: Value,
) -> Result<(), String> {
    observe_security_settings(&settings);
    if maybe_emit_simple_gateway_response(&app, &run_id, &messages, &work_context) {
        return Ok(());
    }
    let api_key = load_api_key(&app, &settings.provider)?;
    let client = model_client_builder(settings.llm_timeout_seconds)
        .build()
        .map_err(|e| e.to_string())?;
    let system = summarize_work_context(&work_context);
    if settings.provider == "anthropic" {
        emit_stream(
            &app,
            &run_id,
            "error",
            json!({"error": "Anthropic streaming tool calls are not wired yet. Disable streaming or use an OpenAI-compatible endpoint."}),
        );
        return Ok(());
    }
    let latest_prompt = effective_user_prompt_from_messages(&messages);
    if settings.provider == "siliconflow"
        && compact_agent_tools_for_prompt(&latest_prompt).is_some()
    {
        let gateway_system = concise_gateway_execution_system(&work_context, &latest_prompt);
        let gateway_messages = vec![ChatMessage {
            role: "user".to_owned(),
            content: latest_prompt.clone(),
            attachments: Vec::new(),
        }];
        emit_stream(
            &app,
            &run_id,
            "started",
            json!({"message": "已收到任务。小亨 Gateway 正在让模型判断执行路径，并会实时显示模型说明、工具调用和每一步结果。"}),
        );
        match complete_openai_chat(
            &app,
            Some(&run_id),
            &client,
            &api_key,
            &settings,
            &gateway_messages,
            &work_context,
            &gateway_system,
        )
        .await
        {
            Ok(content) => {
                let visible_content = strip_hidden_agent_markers(&content);
                emit_stream(
                    &app,
                    &run_id,
                    "done",
                    json!({
                        "content": visible_content,
                        "tasks": [],
                        "workPoolUpdates": {},
                        "todos": [],
                        "toolSteps": []
                    }),
                );
            }
            Err(error) => {
                emit_stream(
                    &app,
                    &run_id,
                    "error",
                    json!({
                        "error": error,
                        "tasks": [],
                        "workPoolUpdates": {},
                        "todos": [],
                        "toolSteps": []
                    }),
                );
            }
        }
        return Ok(());
    }
    let env = StreamChatEnv {
        app: app.clone(),
        run_id: run_id.clone(),
        client: &client,
        api_key: &api_key,
        settings: &settings,
        work_context: &work_context,
        system: &system,
    };
    if let Err(error) = complete_openai_chat_stream(env, &messages).await {
        emit_stream(&app, &run_id, "error", json!({"error": error}));
    }
    Ok(())
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            load_state,
            save_state,
            memory_file_read,
            memory_file_search,
            memory_file_append,
            start_heartbeat_scheduler,
            register_attachment,
            save_api_key,
            has_api_key,
            validate_model_provider,
            complete_chat,
            complete_chat_stream,
            stop_chat_stream,
            archive_task_tree,
            get_runtime_topology,
            list_builtin_tools,
            fetch_marketplace_index,
            fetch_marketplace_aggregated,
            search_skill_market,
            install_market_expert,
            install_market_team,
            install_market_skill,
            install_registry_skill,
            install_skill_from_locator,
            list_installed_market_skills,
            uninstall_market_skill,
            install_default_skills,
            list_im_channels,
            connect_im_channel,
            install_weixin_channel,
            weixin_accounts,
            weixin_login_start,
            weixin_login_poll,
            weixin_monitor_start,
            weixin_send_text,
            tray_notify_attention,
            disconnect_im_channel,
            save_im_channel_config,
            test_im_channel
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Xiaoheng WorkBuddy");
}
