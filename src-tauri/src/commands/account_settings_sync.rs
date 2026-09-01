//! End-to-end encrypted personal settings sync.
//!
//! The marketing service stores only the envelope below. A login password
//! unwraps a random data key locally; the server never receives the password
//! or a plaintext settings value.

use crate::commands::marketing::{apply_cookie, marketing_base_url, marketing_client};
use crate::store::secrets::SecretStore;
use crate::store::AppState;
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::State;
use tokio::sync::Mutex;

const CLOUD_SCHEMA_VERSION: u32 = 1;
const PENDING_UNLOCK_TTL: Duration = Duration::from_secs(5 * 60);

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

#[derive(Debug, Serialize, Deserialize)]
struct CloudSettingsPayload {
    version: u32,
    settings: Value,
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
    let payload = serde_json::to_vec(&CloudSettingsPayload {
        version: CLOUD_SCHEMA_VERSION,
        settings,
    })
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
    let payload: CloudSettingsPayload = serde_json::from_slice(&plaintext)
        .map_err(|_| "云端个人设置内容无效或已损坏".to_string())?;
    if payload.version != CLOUD_SCHEMA_VERSION {
        return Err("云端个人设置版本暂不受支持".to_string());
    }
    let mut settings = state.settings.lock().await;
    settings
        .apply_cloud_snapshot(&payload.settings)
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
}
