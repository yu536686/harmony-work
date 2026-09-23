//! End-to-end encrypted personal settings sync.
//!
//! The marketing service stores only the envelope below. A login password
//! unwraps a random data key locally; the server never receives the password
//! or a plaintext settings value.

use crate::commands::marketing::{apply_cookie, marketing_base_url, marketing_client};
use crate::store::secrets::SecretStore;
use crate::store::settings::CLOUD_EXCLUDED_SETTING_KEYS;
use crate::store::AppState;
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::State;
use tokio::sync::Mutex;

const CLOUD_SCHEMA_VERSION: u32 = 2;
const PENDING_UNLOCK_TTL: Duration = Duration::from_secs(5 * 60);

const CATEGORY_AI_PROVIDER: &str = "aiProvider";
const CATEGORY_VISION: &str = "vision";
const CATEGORY_AGENT: &str = "agent";
const CATEGORY_TOOL_PREFERENCES: &str = "toolPreferences";
const CATEGORY_INTERACTION_SAFETY: &str = "interactionSafety";

#[derive(Debug, Clone, Copy)]
struct CategoryDefinition {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    keys: &'static [&'static str],
}

const CATEGORY_DEFINITIONS: &[CategoryDefinition] = &[
    CategoryDefinition {
        id: CATEGORY_AI_PROVIDER,
        label: "AI 提供商与模型",
        description: "默认提供商、模型、API Key、已启用模型与命名提供商。",
        keys: &[
            "anthropic_api_key",
            "openai_api_key",
            "deepseek_api_key",
            "qwen_api_key",
            "minimax_api_key",
            "zhipu_api_key",
            "kimi_api_key",
            "siliconflow_api_key",
            "harmony_api_key",
            "provider",
            "model",
            "custom_base_url",
            "fallback_models",
            "enabled_models",
            "available_models",
            "llm_providers",
            "max_tokens",
        ],
    },
    CategoryDefinition {
        id: CATEGORY_VISION,
        label: "视觉与多模态",
        description: "视觉模型、视觉 API 配置和主模型复用偏好。",
        keys: &[
            "vision_enabled",
            "vision_use_main_llm",
            "vision_provider",
            "vision_model",
            "vision_api_key",
            "vision_base_url",
        ],
    },
    CategoryDefinition {
        id: CATEGORY_AGENT,
        label: "智能体偏好",
        description: "上下文、执行迭代、压缩、摘要与个人助手偏好。",
        keys: &[
            "context_window",
            "max_iterations",
            "auto_compact_input_tokens_threshold",
            "compaction_micro_percent",
            "compaction_auto_percent",
            "compaction_full_percent",
            "max_tool_result_tokens",
            "summary_model",
            "project_instruction_budget_chars",
            "enable_project_instructions",
            "xiaoheng_personal_prompt",
            "llm_read_timeout_secs",
            "koi_timeout_secs",
            "heartbeat_enabled",
            "heartbeat_interval_mins",
            "heartbeat_prompt",
        ],
    },
    CategoryDefinition {
        id: CATEGORY_TOOL_PREFERENCES,
        label: "工具与技能偏好",
        description: "内置工具开关、技能演进和浏览器执行偏好。",
        keys: &[
            "builtin_tool_enabled",
            "skill_evolution",
            "browser_headless",
            "enable_streaming",
        ],
    },
    CategoryDefinition {
        id: CATEGORY_INTERACTION_SAFETY,
        label: "安全与交互偏好",
        description: "确认策略、语言、界面行为与通用交互设置。",
        keys: &[
            "language",
            "confirm_shell_commands",
            "confirm_file_writes",
            "confirm_mcp_tool_calls",
            "policy_mode",
            "tool_rate_limit_per_minute",
            "allow_multiple_instances",
        ],
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KdfParams {
    algorithm: String,
    salt: String,
    memory_ki_b: u32,
    iterations: u32,
    parallelism: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CipherBlob {
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsEnvelope {
    version: u32,
    kdf: KdfParams,
    wrapped_data_key: CipherBlob,
    payload: CipherBlob,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsSnapshot {
    revision: u64,
    schema_version: u32,
    updated_at: String,
    updated_device_name: String,
    envelope: SettingsEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsMetadata {
    pub revision: u64,
    pub schema_version: u32,
    pub updated_at: String,
    pub updated_device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncStatus {
    pub snapshot: Option<SettingsMetadata>,
    pub locally_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncPrepared {
    pub snapshot_exists: bool,
    pub revision: Option<u64>,
    pub suggested_action: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareAccountSettingsSyncRequest {
    pub user_id: String,
    pub password: String,
    pub device_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncUserRequest {
    pub user_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncSelectedRequest {
    pub user_id: String,
    pub category_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudSettingsPayload {
    version: u32,
    #[serde(default)]
    settings: Option<Value>,
    #[serde(default)]
    categories: BTreeMap<String, Value>,
}

impl CloudSettingsPayload {
    #[cfg(test)]
    fn legacy(settings: Value) -> Self {
        Self {
            version: 1,
            settings: Some(settings),
            categories: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    fn categories(categories: Value) -> Result<Self, String> {
        let categories = serde_json::from_value(categories)
            .map_err(|_| "云端个人设置类别格式无效".to_string())?;
        Ok(Self {
            version: CLOUD_SCHEMA_VERSION,
            settings: None,
            categories,
        })
    }

    fn from_settings(settings: &Value) -> Self {
        Self {
            version: CLOUD_SCHEMA_VERSION,
            settings: None,
            categories: split_cloud_settings(settings),
        }
    }

    fn into_categories(mut self) -> Result<Self, String> {
        match self.version {
            1 => {
                let settings = self.settings.take().unwrap_or_else(|| json!({}));
                self.version = CLOUD_SCHEMA_VERSION;
                self.settings = None;
                self.categories = split_cloud_settings(&settings);
                Ok(self)
            }
            CLOUD_SCHEMA_VERSION => Ok(self),
            _ => Err("云端个人设置版本暂不受支持".to_string()),
        }
    }

    fn category(&self, id: &str) -> &Value {
        static EMPTY_CATEGORY: OnceLock<Value> = OnceLock::new();
        self.categories
            .get(id)
            .unwrap_or_else(|| EMPTY_CATEGORY.get_or_init(|| json!({})))
    }

    fn all_settings(&self) -> Value {
        let mut settings = serde_json::Map::new();
        for value in self.categories.values() {
            if let Value::Object(category) = value {
                for (key, value) in category {
                    if !CLOUD_EXCLUDED_SETTING_KEYS.contains(&key.as_str()) {
                        settings.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Value::Object(settings)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncFieldSummary {
    pub label: String,
    pub configured: bool,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncCategoryPreview {
    pub id: String,
    pub label: String,
    pub description: String,
    pub fields: Vec<AccountSettingsSyncFieldSummary>,
    pub configured_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncCatalog {
    pub categories: Vec<AccountSettingsSyncCategoryPreview>,
    pub local_only_items: Vec<String>,
    pub snapshot: Option<SettingsMetadata>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsSyncPreview {
    pub snapshot: SettingsMetadata,
    pub categories: Vec<AccountSettingsSyncCategoryPreview>,
}

fn category_definition(id: &str) -> Option<&'static CategoryDefinition> {
    CATEGORY_DEFINITIONS
        .iter()
        .find(|definition| definition.id == id)
}

fn category_for_key(key: &str) -> &'static str {
    CATEGORY_DEFINITIONS
        .iter()
        .find(|definition| definition.keys.contains(&key))
        .map(|definition| definition.id)
        .unwrap_or(CATEGORY_AGENT)
}

fn split_cloud_settings(settings: &Value) -> BTreeMap<String, Value> {
    let mut categories = CATEGORY_DEFINITIONS
        .iter()
        .map(|definition| {
            (
                definition.id.to_string(),
                Value::Object(serde_json::Map::new()),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let Value::Object(settings) = settings else {
        return categories;
    };

    for (key, value) in settings {
        if CLOUD_EXCLUDED_SETTING_KEYS.contains(&key.as_str()) {
            continue;
        }
        let category = categories
            .entry(category_for_key(key).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Value::Object(category) = category {
            category.insert(key.clone(), value.clone());
        }
    }
    categories
}

fn validate_category_ids(ids: &[String]) -> Result<Vec<&'static CategoryDefinition>, String> {
    if ids.is_empty() {
        return Err("请至少选择一个可同步配置类别".to_string());
    }
    let mut definitions = Vec::with_capacity(ids.len());
    for id in ids {
        if definitions
            .iter()
            .any(|definition: &&CategoryDefinition| definition.id == id)
        {
            return Err("同步类别不能重复".to_string());
        }
        let definition =
            category_definition(id).ok_or_else(|| format!("不支持的同步类别：{id}"))?;
        definitions.push(definition);
    }
    Ok(definitions)
}

fn merge_selected_categories(
    cloud: CloudSettingsPayload,
    local_settings: &Value,
    category_ids: &[String],
) -> Result<CloudSettingsPayload, String> {
    let definitions = validate_category_ids(category_ids)?;
    let mut cloud = cloud.into_categories()?;
    let local = split_cloud_settings(local_settings);
    for definition in definitions {
        let value = local
            .get(definition.id)
            .cloned()
            .unwrap_or_else(|| json!({}));
        cloud.categories.insert(definition.id.to_string(), value);
    }
    Ok(cloud)
}

fn is_sensitive_key(key: &str) -> bool {
    key.ends_with("_api_key")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("password")
}

fn field_label(key: &str) -> String {
    match key {
        "anthropic_api_key" => "Anthropic API Key".to_string(),
        "openai_api_key" => "OpenAI API Key".to_string(),
        "deepseek_api_key" => "DeepSeek API Key".to_string(),
        "qwen_api_key" => "通义千问 API Key".to_string(),
        "minimax_api_key" => "MiniMax API Key".to_string(),
        "zhipu_api_key" => "智谱 API Key".to_string(),
        "kimi_api_key" => "Kimi API Key".to_string(),
        "siliconflow_api_key" => "SiliconFlow API Key".to_string(),
        "harmony_api_key" => "Harmony API Key".to_string(),
        "vision_api_key" => "视觉 API Key".to_string(),
        _ => key.replace('_', " "),
    }
}

fn value_is_configured(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_u64().unwrap_or_default() != 0,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn category_preview(
    id: &str,
    settings: &Value,
) -> Result<AccountSettingsSyncCategoryPreview, String> {
    let definition = category_definition(id).ok_or_else(|| format!("不支持的同步类别：{id}"))?;
    let Value::Object(settings) = settings else {
        return Ok(AccountSettingsSyncCategoryPreview {
            id: definition.id.to_string(),
            label: definition.label.to_string(),
            description: definition.description.to_string(),
            fields: Vec::new(),
            configured_fields: Vec::new(),
        });
    };
    let fields = settings
        .iter()
        .map(|(key, value)| AccountSettingsSyncFieldSummary {
            label: field_label(key),
            configured: value_is_configured(value),
            sensitive: is_sensitive_key(key),
        })
        .collect::<Vec<_>>();
    let configured_fields = fields
        .iter()
        .filter(|field| field.configured)
        .map(|field| field.label.clone())
        .collect();
    Ok(AccountSettingsSyncCategoryPreview {
        id: definition.id.to_string(),
        label: definition.label.to_string(),
        description: definition.description.to_string(),
        fields,
        configured_fields,
    })
}

fn all_category_previews(
    payload: &CloudSettingsPayload,
) -> Result<Vec<AccountSettingsSyncCategoryPreview>, String> {
    CATEGORY_DEFINITIONS
        .iter()
        .map(|definition| category_preview(definition.id, payload.category(definition.id)))
        .collect()
}

fn selected_settings(
    payload: &CloudSettingsPayload,
    category_ids: &[String],
) -> Result<Value, String> {
    let definitions = validate_category_ids(category_ids)?;
    let mut settings = serde_json::Map::new();
    for definition in definitions {
        let Value::Object(category) = payload.category(definition.id) else {
            continue;
        };
        for (key, value) in category {
            if !CLOUD_EXCLUDED_SETTING_KEYS.contains(&key.as_str()) {
                settings.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(settings))
}

struct PendingUnlock {
    data_key: [u8; 32],
    kdf: KdfParams,
    wrapped_data_key: CipherBlob,
    device_name: String,
    expires_at: Instant,
}

static PENDING_UNLOCKS: OnceLock<Mutex<HashMap<String, PendingUnlock>>> = OnceLock::new();

fn pending_unlocks() -> &'static Mutex<HashMap<String, PendingUnlock>> {
    PENDING_UNLOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn app_data_dir() -> PathBuf {
    crate::app::logging::default_app_data_dir()
}

fn local_data_key_path(user_id: &str) -> PathBuf {
    app_data_dir().join(format!("account-settings-vault-{user_id}.key"))
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("无法生成加密随机数：{error}"))?;
    Ok(bytes)
}

fn derive_password_key(password: &str, kdf: &KdfParams) -> Result<[u8; 32], String> {
    let salt = BASE64
        .decode(&kdf.salt)
        .map_err(|_| "云端加密盐格式无效".to_string())?;
    let params = Params::new(kdf.memory_ki_b, kdf.iterations, kdf.parallelism, Some(32))
        .map_err(|error| format!("Argon2 参数无效：{error}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|_| "无法使用该登录密码解锁个人设置".to_string())?;
    Ok(key)
}

fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<CipherBlob, String> {
    let nonce = random_bytes::<24>()?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| "个人设置加密失败".to_string())?;
    Ok(CipherBlob {
        nonce: BASE64.encode(nonce),
        ciphertext: BASE64.encode(ciphertext),
    })
}

fn decrypt(key: &[u8; 32], blob: &CipherBlob) -> Result<Vec<u8>, String> {
    let nonce = BASE64
        .decode(&blob.nonce)
        .map_err(|_| "云端加密随机数格式无效".to_string())?;
    let nonce: [u8; 24] = nonce
        .try_into()
        .map_err(|_| "云端加密随机数长度无效".to_string())?;
    let ciphertext = BASE64
        .decode(&blob.ciphertext)
        .map_err(|_| "云端密文格式无效".to_string())?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "无法解锁个人设置：登录密码或云端密文不匹配".to_string())
}

fn new_kdf() -> Result<KdfParams, String> {
    Ok(KdfParams {
        algorithm: "argon2id".to_string(),
        salt: BASE64.encode(random_bytes::<32>()?),
        memory_ki_b: 65_536,
        iterations: 3,
        parallelism: 1,
    })
}

fn save_local_data_key(user_id: &str, data_key: &[u8; 32]) -> Result<(), String> {
    let app_dir = app_data_dir();
    std::fs::create_dir_all(&app_dir).map_err(|error| error.to_string())?;
    let store = SecretStore::new(&app_dir).map_err(|error| error.to_string())?;
    let encrypted = store
        .encrypt_hex(&BASE64.encode(data_key))
        .map_err(|error| error.to_string())?;
    std::fs::write(local_data_key_path(user_id), encrypted).map_err(|error| error.to_string())
}

fn load_local_data_key(user_id: &str) -> Result<[u8; 32], String> {
    let encrypted = std::fs::read_to_string(local_data_key_path(user_id))
        .map_err(|_| "该设备尚未完成个人设置加密初始化，请重新登录后选择同步".to_string())?;
    let store = SecretStore::new(&app_data_dir()).map_err(|error| error.to_string())?;
    let encoded = store
        .decrypt_hex(encrypted.trim())
        .map_err(|_| "该设备的个人设置密钥不可用，请重新登录后选择同步".to_string())?;
    let data_key = BASE64
        .decode(encoded)
        .map_err(|_| "该设备的个人设置密钥格式无效，请重新登录后选择同步".to_string())?;
    data_key
        .try_into()
        .map_err(|_| "该设备的个人设置密钥长度无效，请重新登录后选择同步".to_string())
}

async fn get_json(path: &str) -> Result<Value, String> {
    let client = marketing_client()?;
    let request = apply_cookie(client.get(format!("{}{}", marketing_base_url(), path))).await;
    let response = request
        .send()
        .await
        .map_err(|error| format!("个人设置云同步请求失败：{error}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("个人设置云同步返回 HTTP {status}: {body}"));
    }
    serde_json::from_str(&body).map_err(|error| format!("个人设置云同步响应无效：{error}"))
}

async fn put_json(path: &str, body: Value) -> Result<Value, String> {
    let client = marketing_client()?;
    let request = apply_cookie(
        client
            .put(format!("{}{}", marketing_base_url(), path))
            .json(&body),
    )
    .await;
    let response = request
        .send()
        .await
        .map_err(|error| format!("个人设置云同步请求失败：{error}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("个人设置云同步返回 HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|error| format!("个人设置云同步响应无效：{error}"))
}

async fn delete_remote_snapshot() -> Result<(), String> {
    let client = marketing_client()?;
    let request = apply_cookie(client.delete(format!(
        "{}/api/workbuddy/settings/snapshot",
        marketing_base_url()
    )))
    .await;
    let response = request
        .send()
        .await
        .map_err(|error| format!("个人设置云同步请求失败：{error}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("删除云端个人设置失败：HTTP {}", response.status()))
    }
}

async fn remote_snapshot() -> Result<Option<SettingsSnapshot>, String> {
    let value = get_json("/api/workbuddy/settings/snapshot").await?;
    serde_json::from_value(value.get("snapshot").cloned().unwrap_or(Value::Null))
        .map(Some)
        .or_else(|error| {
            if value.get("snapshot").is_some_and(Value::is_null) {
                Ok(None)
            } else {
                Err(format!("云端个人设置格式无效：{error}"))
            }
        })
}

async fn take_pending_if_valid(user_id: &str) -> Option<PendingUnlock> {
    let mut pending = pending_unlocks().lock().await;
    let unlock = pending.remove(user_id)?;
    if unlock.expires_at <= Instant::now() {
        return None;
    }
    Some(unlock)
}

async fn backup_material(user_id: &str) -> Result<PendingUnlock, String> {
    if let Some(unlock) = take_pending_if_valid(user_id).await {
        return Ok(unlock);
    }

    // After the initial login confirmation, the random data key stays protected
    // locally. It lets later Settings saves refresh the encrypted payload without
    // retaining the user's password in memory or on disk.
    let snapshot = remote_snapshot()
        .await?
        .ok_or_else(|| "请重新登录后选择首次加密备份".to_string())?;
    Ok(PendingUnlock {
        data_key: load_local_data_key(user_id)?,
        kdf: snapshot.envelope.kdf,
        wrapped_data_key: snapshot.envelope.wrapped_data_key,
        device_name: "WorkBuddy desktop".to_string(),
        expires_at: Instant::now(),
    })
}

#[tauri::command]
pub async fn account_settings_sync_status(
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncStatus, String> {
    let value = get_json("/api/workbuddy/settings/meta").await?;
    let snapshot =
        serde_json::from_value(value.get("snapshot").cloned().unwrap_or(Value::Null)).ok();
    let locally_configured = state.settings.lock().await.is_configured();
    Ok(AccountSettingsSyncStatus {
        snapshot,
        locally_configured,
    })
}

#[tauri::command]
pub async fn account_settings_sync_catalog(
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncCatalog, String> {
    let value = get_json("/api/workbuddy/settings/meta").await?;
    let snapshot =
        serde_json::from_value(value.get("snapshot").cloned().unwrap_or(Value::Null)).ok();
    let settings = state.settings.lock().await.cloud_snapshot();
    let payload = CloudSettingsPayload::from_settings(&settings);
    Ok(AccountSettingsSyncCatalog {
        categories: all_category_previews(&payload)?,
        local_only_items: vec![
            "工作区与工作区外文件访问权限".to_string(),
            "IM、邮件和 Webhook 渠道凭据".to_string(),
            "MCP、SSH 与用户工具连接".to_string(),
            "运行环境路径与窗口位置".to_string(),
        ],
        snapshot,
    })
}

#[tauri::command]
pub async fn account_settings_sync_preview(
    request: AccountSettingsSyncUserRequest,
) -> Result<Option<AccountSettingsSyncPreview>, String> {
    let Some(snapshot) = remote_snapshot().await? else {
        return Ok(None);
    };
    let data_key = match take_pending_if_valid(&request.user_id).await {
        Some(unlock) => unlock.data_key,
        None => load_local_data_key(&request.user_id)?,
    };
    let plaintext = decrypt(&data_key, &snapshot.envelope.payload)?;
    let payload: CloudSettingsPayload = serde_json::from_slice::<CloudSettingsPayload>(&plaintext)
        .map_err(|_| "云端个人设置内容无效或已损坏".to_string())?
        .into_categories()?;
    Ok(Some(AccountSettingsSyncPreview {
        snapshot: SettingsMetadata {
            revision: snapshot.revision,
            schema_version: snapshot.schema_version,
            updated_at: snapshot.updated_at,
            updated_device_name: snapshot.updated_device_name,
        },
        categories: all_category_previews(&payload)?,
    }))
}

#[tauri::command]
pub async fn account_settings_sync_prepare(
    request: PrepareAccountSettingsSyncRequest,
) -> Result<AccountSettingsSyncPrepared, String> {
    let snapshot = remote_snapshot().await?;
    let (data_key, kdf, wrapped_data_key, revision, suggested_action) =
        if let Some(snapshot) = snapshot {
            let password_key = derive_password_key(&request.password, &snapshot.envelope.kdf)?;
            let raw_data_key = decrypt(&password_key, &snapshot.envelope.wrapped_data_key)?;
            let data_key: [u8; 32] = raw_data_key
                .try_into()
                .map_err(|_| "云端个人设置密钥长度无效".to_string())?;
            (
                data_key,
                snapshot.envelope.kdf,
                snapshot.envelope.wrapped_data_key,
                Some(snapshot.revision),
                "restore".to_string(),
            )
        } else {
            let kdf = new_kdf()?;
            let password_key = derive_password_key(&request.password, &kdf)?;
            let data_key = random_bytes::<32>()?;
            let wrapped_data_key = encrypt(&password_key, &data_key)?;
            (data_key, kdf, wrapped_data_key, None, "backup".to_string())
        };
    save_local_data_key(&request.user_id, &data_key)?;
    pending_unlocks().lock().await.insert(
        request.user_id,
        PendingUnlock {
            data_key,
            kdf,
            wrapped_data_key,
            device_name: request.device_name.trim().chars().take(80).collect(),
            expires_at: Instant::now() + PENDING_UNLOCK_TTL,
        },
    );
    Ok(AccountSettingsSyncPrepared {
        snapshot_exists: revision.is_some(),
        revision,
        suggested_action,
    })
}

#[tauri::command]
pub async fn account_settings_sync_backup(
    request: AccountSettingsSyncUserRequest,
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncStatus, String> {
    let pending = backup_material(&request.user_id).await?;
    let settings = state.settings.lock().await.cloud_snapshot();
    let payload = serde_json::to_vec(&CloudSettingsPayload::from_settings(&settings))
        .map_err(|error| format!("无法序列化个人设置：{error}"))?;
    let payload = encrypt(&pending.data_key, &payload)?;
    let metadata = get_json("/api/workbuddy/settings/meta").await?;
    let base_revision = metadata
        .get("snapshot")
        .and_then(|value| value.get("revision"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    put_json(
        "/api/workbuddy/settings/snapshot",
        json!({
            "baseRevision": base_revision,
            "schemaVersion": CLOUD_SCHEMA_VERSION,
            "deviceName": pending.device_name,
            "envelope": {
                "version": CLOUD_SCHEMA_VERSION,
                "kdf": pending.kdf,
                "wrappedDataKey": pending.wrapped_data_key,
                "payload": payload,
            }
        }),
    )
    .await?;
    account_settings_sync_status(state).await
}

#[tauri::command]
pub async fn account_settings_sync_backup_selected(
    request: AccountSettingsSyncSelectedRequest,
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncStatus, String> {
    validate_category_ids(&request.category_ids)?;
    let pending = backup_material(&request.user_id).await?;
    let remote = remote_snapshot().await?;
    let base_revision = remote
        .as_ref()
        .map(|snapshot| snapshot.revision)
        .unwrap_or(0);
    let cloud_payload = match remote {
        Some(snapshot) => {
            let plaintext = decrypt(&pending.data_key, &snapshot.envelope.payload)?;
            serde_json::from_slice::<CloudSettingsPayload>(&plaintext)
                .map_err(|_| "云端个人设置内容无效或已损坏".to_string())?
                .into_categories()?
        }
        None => CloudSettingsPayload::from_settings(&json!({})),
    };
    let local_settings = state.settings.lock().await.cloud_snapshot();
    let merged = merge_selected_categories(cloud_payload, &local_settings, &request.category_ids)?;
    let payload = encrypt(
        &pending.data_key,
        &serde_json::to_vec(&merged).map_err(|error| format!("无法序列化个人设置：{error}"))?,
    )?;
    put_json(
        "/api/workbuddy/settings/snapshot",
        json!({
            "baseRevision": base_revision,
            "schemaVersion": CLOUD_SCHEMA_VERSION,
            "deviceName": pending.device_name,
            "envelope": {
                "version": CLOUD_SCHEMA_VERSION,
                "kdf": pending.kdf,
                "wrappedDataKey": pending.wrapped_data_key,
                "payload": payload,
            }
        }),
    )
    .await
    .map_err(|error| {
        if error.contains("HTTP 409") {
            "云端配置已在其他设备更新，请重新读取后重试".to_string()
        } else {
            error
        }
    })?;
    account_settings_sync_status(state).await
}

#[tauri::command]
pub async fn account_settings_sync_restore(
    request: AccountSettingsSyncUserRequest,
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncStatus, String> {
    let Some(snapshot) = remote_snapshot().await? else {
        return Err("账号下没有可恢复的个人设置".to_string());
    };
    let data_key = match take_pending_if_valid(&request.user_id).await {
        Some(unlock) => unlock.data_key,
        None => load_local_data_key(&request.user_id)?,
    };
    let plaintext = decrypt(&data_key, &snapshot.envelope.payload)?;
    let payload: CloudSettingsPayload = serde_json::from_slice::<CloudSettingsPayload>(&plaintext)
        .map_err(|_| "云端个人设置内容无效或已损坏".to_string())?;
    let payload = payload.into_categories()?;
    let mut settings = state.settings.lock().await;
    settings
        .apply_cloud_snapshot(&payload.all_settings())
        .map_err(|error| error.to_string())?;
    settings.save().map_err(|error| error.to_string())?;
    crate::agent_core::approval::sync_confirm_flags(
        &state.confirm_flags,
        settings.confirm_shell_commands,
        settings.confirm_file_writes,
    );
    drop(settings);
    account_settings_sync_status(state).await
}

#[tauri::command]
pub async fn account_settings_sync_restore_selected(
    request: AccountSettingsSyncSelectedRequest,
    state: State<'_, AppState>,
) -> Result<AccountSettingsSyncStatus, String> {
    validate_category_ids(&request.category_ids)?;
    let Some(snapshot) = remote_snapshot().await? else {
        return Err("账号下没有可恢复的个人设置".to_string());
    };
    let data_key = match take_pending_if_valid(&request.user_id).await {
        Some(unlock) => unlock.data_key,
        None => load_local_data_key(&request.user_id)?,
    };
    let plaintext = decrypt(&data_key, &snapshot.envelope.payload)?;
    let payload: CloudSettingsPayload = serde_json::from_slice::<CloudSettingsPayload>(&plaintext)
        .map_err(|_| "云端个人设置内容无效或已损坏".to_string())?
        .into_categories()?;
    let selected = selected_settings(&payload, &request.category_ids)?;
    let mut settings = state.settings.lock().await;
    settings
        .apply_cloud_snapshot(&selected)
        .map_err(|error| error.to_string())?;
    settings.save().map_err(|error| error.to_string())?;
    crate::agent_core::approval::sync_confirm_flags(
        &state.confirm_flags,
        settings.confirm_shell_commands,
        settings.confirm_file_writes,
    );
    drop(settings);
    account_settings_sync_status(state).await
}

#[tauri::command]
pub async fn account_settings_sync_delete(
    request: AccountSettingsSyncUserRequest,
) -> Result<(), String> {
    delete_remote_snapshot().await?;
    pending_unlocks().lock().await.remove(&request.user_id);
    let _ = std::fs::remove_file(local_data_key_path(&request.user_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_a_cloud_payload_with_a_random_data_key() {
        let key = [7u8; 32];
        let encrypted = encrypt(&key, b"provider settings").expect("encrypt");
        assert_ne!(encrypted.ciphertext, "provider settings");
        assert_eq!(
            decrypt(&key, &encrypted).expect("decrypt"),
            b"provider settings"
        );
    }

    #[test]
    fn password_key_derivation_is_stable_for_the_same_argon2_parameters() {
        let kdf = KdfParams {
            algorithm: "argon2id".to_string(),
            salt: BASE64.encode([3u8; 32]),
            memory_ki_b: 65_536,
            iterations: 3,
            parallelism: 1,
        };
        assert_eq!(
            derive_password_key("same-password", &kdf).unwrap(),
            derive_password_key("same-password", &kdf).unwrap()
        );
        assert_ne!(
            derive_password_key("same-password", &kdf).unwrap(),
            derive_password_key("other-password", &kdf).unwrap()
        );
    }

    #[test]
    fn legacy_snapshot_migrates_into_named_categories() {
        let payload = CloudSettingsPayload::legacy(json!({
            "provider": "deepseek",
            "deepseek_api_key": "secret",
            "context_window": 128000,
            "workspace_root": "C:\\local"
        }));

        let migrated = payload.into_categories().expect("migrate legacy payload");
        assert_eq!(migrated.category("aiProvider")["provider"], "deepseek");
        assert_eq!(migrated.category("agent")["context_window"], 128000);
        assert!(migrated.all_settings().get("workspace_root").is_none());
    }

    #[test]
    fn merging_selected_categories_preserves_unselected_cloud_categories() {
        let cloud = CloudSettingsPayload::categories(json!({
            "aiProvider": { "model": "remote-model" },
            "agent": { "context_window": 64000 }
        }))
        .expect("cloud categories");
        let local = json!({ "model": "local-model", "context_window": 128000 });

        let merged = merge_selected_categories(cloud, &local, &["aiProvider".to_string()])
            .expect("merge selected category");
        assert_eq!(merged.category("aiProvider")["model"], "local-model");
        assert_eq!(merged.category("agent")["context_window"], 64000);
    }

    #[test]
    fn category_preview_masks_api_key_values() {
        let preview = category_preview(
            "aiProvider",
            &json!({
                "deepseek_api_key": "sk-secret"
            }),
        )
        .expect("preview");

        assert_eq!(preview.configured_fields, vec!["DeepSeek API Key"]);
        assert!(!format!("{preview:?}").contains("sk-secret"));
    }
}
