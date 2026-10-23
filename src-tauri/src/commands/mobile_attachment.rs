//! Local file registration for mobile conversations. Paths never leave this desktop.
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

pub(crate) const MAX_MOBILE_ATTACHMENT_BYTES: u64 = 200 * 1024 * 1024;
const TRANSFER_CHUNK_BYTES: u64 = 16 * 1024;

struct TransferGrant {
    device_auth: (String, String),
    mobile_device_id: String,
    file: File,
    record: PreparedAttachment,
    next_offset: u64,
    checked_at: Instant,
    checked_bytes: u64,
}

static TRANSFER_GRANTS: OnceLock<Mutex<HashMap<String, TransferGrant>>> = OnceLock::new();
fn grants() -> &'static Mutex<HashMap<String, TransferGrant>> {
    TRANSFER_GRANTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn clear_transfer_grants() {
    if let Ok(mut active) = grants().lock() {
        active.clear();
    }
}

pub(crate) fn revoke_phone_transfers(mobile_device_id: &str) {
    if let Ok(mut active) = grants().lock() {
        active.retain(|_, grant| grant.mobile_device_id != mobile_device_id);
    }
}

fn read_granted_chunk(
    grant: &mut TransferGrant,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, String> {
    if length == 0
        || length > TRANSFER_CHUNK_BYTES
        || offset != grant.next_offset
        || offset >= grant.record.size
        || length > grant.record.size - offset
    {
        return Err("Invalid file block request".into());
    }
    let metadata = grant
        .file
        .metadata()
        .map_err(|_| "Attachment file is unavailable".to_string())?;
    let modified = metadata
        .modified()
        .map_err(|_| "Attachment file changed".to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Attachment file changed".to_string())?
        .as_nanos();
    if metadata.len() != grant.record.size || modified != grant.record.modified_ns {
        return Err("Attachment file changed".into());
    }
    grant
        .file
        .seek(SeekFrom::Start(offset))
        .map_err(|_| "Cannot seek attachment file".to_string())?;
    let mut bytes = vec![0_u8; length as usize];
    grant
        .file
        .read_exact(&mut bytes)
        .map_err(|_| "Cannot read attachment file".to_string())?;
    grant.next_offset += length;
    grant.checked_bytes += length;
    Ok(bytes)
}

fn open_verified_file(record: &PreparedAttachment) -> Result<File, String> {
    let mut file =
        File::open(&record.path).map_err(|_| "Attachment file is unavailable".to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| "Attachment file is unavailable".to_string())?;
    let modified = metadata
        .modified()
        .map_err(|_| "Attachment file changed".to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Attachment file changed".to_string())?
        .as_nanos();
    if metadata.len() != record.size || modified != record.modified_ns {
        return Err("Attachment file changed".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Cannot read attachment file".to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| "Attachment file changed".to_string())?;
    let after_modified = after
        .modified()
        .map_err(|_| "Attachment file changed".to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Attachment file changed".to_string())?
        .as_nanos();
    if after.len() != record.size
        || after_modified != record.modified_ns
        || hex::encode(digest.finalize()) != record.sha256
    {
        return Err("Attachment file changed".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot seek attachment file".to_string())?;
    Ok(file)
}

#[tauri::command]
pub async fn mobile_attachment_authorize(
    app: AppHandle,
    attachment_id: String,
    mobile_device_id: String,
    ticket: String,
    offset: u64,
) -> Result<Value, String> {
    let device_auth = crate::commands::mobile_bridge::current_device_auth()
        .ok_or("Please reconnect this desktop")?;
    let id = uuid::Uuid::parse_str(&attachment_id)
        .map_err(|_| "Invalid attachment ID".to_string())?
        .to_string();
    uuid::Uuid::parse_str(&mobile_device_id).map_err(|_| "Invalid mobile device ID".to_string())?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "Attachment store unavailable".to_string())?;
    let inspected = tokio::task::spawn_blocking(move || {
        let record = load_record(&directory, &id)?;
        if offset > record.size {
            return Err("Invalid transfer offset".into());
        }
        let file = open_verified_file(&record)?;
        Ok::<_, String>((record, file))
    })
    .await
    .map_err(|_| "Attachment inspection stopped unexpectedly".to_string())?;
    let (record, file) = match inspected {
        Ok(value) => value,
        Err(error) => {
            if error != "Invalid transfer offset" {
                let _ = crate::commands::mobile_bridge::revoke_attachment(&attachment_id).await;
            }
            return Err(error);
        }
    };
    let verification = crate::commands::mobile_bridge::verify_attachment_ticket(
        &attachment_id,
        &ticket,
        &mobile_device_id,
    )
    .await?;
    let attachment = &verification["attachment"];
    if attachment["id"].as_str() != Some(attachment_id.as_str())
        || attachment["mobileDeviceId"].as_str() != Some(mobile_device_id.as_str())
        || attachment["size"].as_u64() != Some(record.size)
        || attachment["sha256"].as_str() != Some(record.sha256.as_str())
    {
        return Err("Attachment metadata no longer matches the local file".into());
    }
    let mut active = grants()
        .lock()
        .map_err(|_| "Transfer state unavailable".to_string())?;
    if !crate::commands::mobile_bridge::device_auth_matches(&device_auth) {
        return Err("Desktop device changed".into());
    }
    active.retain(|_, grant| grant.checked_at.elapsed() < Duration::from_secs(300));
    active.insert(
        attachment_id,
        TransferGrant {
            device_auth,
            mobile_device_id,
            file,
            record,
            next_offset: offset,
            checked_at: Instant::now(),
            checked_bytes: 0,
        },
    );
    Ok(attachment.clone())
}

#[tauri::command]
pub async fn mobile_attachment_read_chunk(
    attachment_id: String,
    offset: u64,
    length: u64,
) -> Result<String, String> {
    uuid::Uuid::parse_str(&attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    let check_status = {
        let mut active = grants()
            .lock()
            .map_err(|_| "Transfer state unavailable".to_string())?;
        let grant = active
            .get(&attachment_id)
            .ok_or("No authorized file transfer")?;
        if !crate::commands::mobile_bridge::device_auth_matches(&grant.device_auth) {
            active.remove(&attachment_id);
            return Err("Desktop device changed".into());
        }
        grant.checked_bytes >= 1024 * 1024 || grant.checked_at.elapsed() >= Duration::from_secs(10)
    };
    if check_status {
        if crate::commands::mobile_bridge::attachment_status(&attachment_id)
            .await
            .is_err()
        {
            grants()
                .lock()
                .map_err(|_| "Transfer state unavailable".to_string())?
                .remove(&attachment_id);
            return Err("Attachment is no longer available".into());
        }
        if let Some(grant) = grants()
            .lock()
            .map_err(|_| "Transfer state unavailable".to_string())?
            .get_mut(&attachment_id)
        {
            grant.checked_at = Instant::now();
            grant.checked_bytes = 0;
        }
    }
    let result = {
        let mut active = grants()
            .lock()
            .map_err(|_| "Transfer state unavailable".to_string())?;
        let grant = active
            .get_mut(&attachment_id)
            .ok_or("No authorized file transfer")?;
        if !crate::commands::mobile_bridge::device_auth_matches(&grant.device_auth) {
            active.remove(&attachment_id);
            return Err("Desktop device changed".into());
        }
        read_granted_chunk(grant, offset, length)
    };
    match result {
        Ok(bytes) => Ok(STANDARD.encode(bytes)),
        Err(error) => {
            if error != "Invalid file block request" {
                grants()
                    .lock()
                    .map_err(|_| "Transfer state unavailable".to_string())?
                    .remove(&attachment_id);
                let _ = crate::commands::mobile_bridge::revoke_attachment(&attachment_id).await;
            }
            Err(error)
        }
    }
}

#[tauri::command]
pub fn mobile_attachment_cancel(attachment_id: String) -> Result<(), String> {
    uuid::Uuid::parse_str(&attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    grants()
        .lock()
        .map_err(|_| "Transfer state unavailable".to_string())?
        .remove(&attachment_id);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedAttachment {
    pub path: PathBuf,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub sha256: String,
    pub modified_ns: u128,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ShareRequest {
    pub path: PathBuf,
    pub display_name: String,
}

pub(crate) fn parse_share_request(raw: &[u8]) -> Result<ShareRequest, String> {
    if raw.len() > 8 * 1024 {
        return Err("File share request is too large".into());
    }
    let request: ShareRequest =
        serde_json::from_slice(raw).map_err(|_| "Invalid file share request".to_string())?;
    if !request.path.is_absolute() || request.path.as_os_str().len() > 4096 {
        return Err("File share path must be absolute".into());
    }
    let name = request.display_name.trim();
    if name.is_empty()
        || name.chars().count() > 120
        || name
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control())
    {
        return Err("Invalid file share display name".into());
    }
    Ok(request)
}

pub(crate) fn save_record(
    app_data_dir: &Path,
    attachment_id: &str,
    record: &PreparedAttachment,
) -> Result<(), String> {
    let id =
        uuid::Uuid::parse_str(attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    let directory = app_data_dir.join("mobile_attachments");
    std::fs::create_dir_all(&directory)
        .map_err(|_| "Cannot create local attachment store".to_string())?;
    let destination = directory.join(format!("{id}.json"));
    let temporary = directory.join(format!("{id}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(
        &temporary,
        serde_json::to_vec(record).map_err(|_| "Cannot encode attachment".to_string())?,
    )
    .map_err(|_| "Cannot save local attachment".to_string())?;
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("Cannot commit local attachment: {error}"));
    }
    Ok(())
}

pub(crate) fn load_record(
    app_data_dir: &Path,
    attachment_id: &str,
) -> Result<PreparedAttachment, String> {
    let id =
        uuid::Uuid::parse_str(attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    let path = app_data_dir
        .join("mobile_attachments")
        .join(format!("{id}.json"));
    let metadata = path
        .metadata()
        .map_err(|_| "Local attachment not found".to_string())?;
    if metadata.len() > 16 * 1024 {
        return Err("Invalid local attachment record".into());
    }
    let record: PreparedAttachment = serde_json::from_slice(
        &std::fs::read(path).map_err(|_| "Cannot read local attachment".to_string())?,
    )
    .map_err(|_| "Invalid local attachment record".to_string())?;
    if !record.path.is_absolute()
        || record.size > MAX_MOBILE_ATTACHMENT_BYTES
        || record.sha256.len() != 64
        || !record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Invalid local attachment record".into());
    }
    Ok(record)
}

pub(crate) fn validate_saved_file(record: &PreparedAttachment) -> Result<(), String> {
    let current = prepare_file(&record.path, &record.name, Path::new(""), true)?;
    if current.path != record.path
        || current.size != record.size
        || current.modified_ns != record.modified_ns
        || current.sha256 != record.sha256
    {
        return Err("Attachment file changed or moved".into());
    }
    Ok(())
}

pub(crate) fn create_request_context(app_data_dir: &Path) -> Result<(String, PathBuf), String> {
    let parent = app_data_dir.join("mobile_share_requests");
    std::fs::create_dir_all(&parent)
        .map_err(|_| "Cannot create mobile task request store".to_string())?;
    for _ in 0..4 {
        let mut random = [0_u8; 32];
        getrandom::getrandom(&mut random)
            .map_err(|_| "Cannot create mobile task capability".to_string())?;
        let capability = URL_SAFE_NO_PAD.encode(random);
        let directory = parent.join(&capability);
        if std::fs::create_dir(&directory).is_ok() {
            let active = parent.join("active");
            if let Err(error) = std::fs::write(&active, &capability) {
                let _ = std::fs::remove_dir(&directory);
                return Err(format!("Cannot activate mobile task file sharing: {error}"));
            }
            return Ok((capability, directory));
        }
    }
    Err("Cannot allocate mobile task request store".into())
}

pub(crate) fn clear_request_context(directory: &Path) {
    if let Some(parent) = directory.parent() {
        let active = parent.join("active");
        if std::fs::read_to_string(&active).ok().as_deref()
            == directory.file_name().and_then(|name| name.to_str())
        {
            let _ = std::fs::remove_file(active);
        }
    }
    let _ = std::fs::remove_dir_all(directory);
}

pub(crate) fn take_pending_request(
    directory: &Path,
) -> Result<Option<(PathBuf, ShareRequest)>, String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|_| "Mobile task request store is unavailable".to_string())?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for request_path in entries {
        let Some(name) = request_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".request.json") else {
            continue;
        };
        if uuid::Uuid::parse_str(id).is_err() {
            continue;
        }
        let processing = directory.join(format!("{id}.processing.json"));
        if std::fs::rename(&request_path, &processing).is_err() {
            continue;
        }
        let parsed = std::fs::read(&processing)
            .map_err(|_| "Cannot read mobile file request".to_string())
            .and_then(|raw| parse_share_request(&raw));
        match parsed {
            Ok(request) => return Ok(Some((processing, request))),
            Err(error) => {
                let _ = write_response(&processing, &serde_json::json!({"error": error}));
            }
        }
    }
    Ok(None)
}

pub(crate) fn write_response(processing: &Path, response: &Value) -> Result<(), String> {
    let name = processing
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid mobile file request".to_string())?;
    let id = name
        .strip_suffix(".processing.json")
        .ok_or_else(|| "Invalid mobile file request".to_string())?;
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid mobile file request".to_string())?;
    let directory = processing
        .parent()
        .ok_or_else(|| "Invalid mobile file request".to_string())?;
    let temporary = directory.join(format!("{id}.reply.tmp"));
    let destination = directory.join(format!("{id}.response.json"));
    std::fs::write(
        &temporary,
        serde_json::to_vec(response).map_err(|_| "Cannot encode file share result".to_string())?,
    )
    .map_err(|_| "Cannot write file share result".to_string())?;
    std::fs::rename(&temporary, &destination)
        .map_err(|_| "Cannot publish file share result".to_string())?;
    let _ = std::fs::remove_file(processing);
    Ok(())
}

#[tauri::command]
pub async fn mobile_attachment_open(app: AppHandle, attachment_id: String) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "Attachment store unavailable".to_string())?;
    let path = tokio::task::spawn_blocking(move || {
        let record = load_record(&app_data_dir, &attachment_id)?;
        validate_saved_file(&record)?;
        Ok::<PathBuf, String>(record.path)
    })
    .await
    .map_err(|_| "Attachment inspection stopped unexpectedly".to_string())??;
    crate::app::bootstrap::open_path(path.to_string_lossy().into_owned())
}

pub(crate) fn prepare_file(
    path: &Path,
    display_name: &str,
    workspace_root: &Path,
    allow_outside_workspace: bool,
) -> Result<PreparedAttachment, String> {
    let name = display_name.trim();
    if name.is_empty()
        || name.chars().count() > 120
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control())
    {
        return Err("Invalid attachment display name".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "File not found".to_string())?;
    if !allow_outside_workspace {
        let root = workspace_root
            .canonicalize()
            .map_err(|_| "Workspace is unavailable".to_string())?;
        if !canonical.starts_with(root) {
            return Err("File is outside the authorized workspace".into());
        }
    }
    let metadata = canonical
        .metadata()
        .map_err(|_| "Cannot inspect file".to_string())?;
    if !metadata.is_file() {
        return Err("Only regular files can be shared".into());
    }
    if metadata.len() > MAX_MOBILE_ATTACHMENT_BYTES {
        return Err("File exceeds the 200 MiB mobile transfer limit".into());
    }
    let modified_ns = metadata
        .modified()
        .map_err(|_| "Cannot inspect file timestamp".to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid file timestamp".to_string())?
        .as_nanos();
    let mut file = File::open(&canonical).map_err(|_| "Cannot open file".to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "Cannot read file".to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let after = canonical
        .metadata()
        .map_err(|_| "File changed during registration".to_string())?;
    let after_ns = after
        .modified()
        .map_err(|_| "File changed during registration".to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "File changed during registration".to_string())?
        .as_nanos();
    if after.len() != metadata.len() || after_ns != modified_ns {
        return Err("File changed during registration".into());
    }
    let mime_type = match canonical
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "txt" | "md" | "log" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .to_string();
    Ok(PreparedAttachment {
        path: canonical,
        name: name.to_string(),
        mime_type,
        size: metadata.len(),
        sha256: hex::encode(digest.finalize()),
        modified_ns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_attachment_accepts_a_workspace_file_and_hashes_its_bytes() {
        let root = std::env::temp_dir().join(format!("mobile-attachment-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("report.txt");
        std::fs::write(&path, b"test").unwrap();
        let prepared = prepare_file(&path, "report.txt", &root, false).unwrap();
        assert_eq!(prepared.name, "report.txt");
        assert_eq!(prepared.mime_type, "text/plain");
        assert_eq!(prepared.size, 4);
        assert_eq!(
            prepared.sha256,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_rejects_parent_traversal_and_oversize_files() {
        let root = std::env::temp_dir().join(format!("mobile-attachment-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let outside = root.join("outside.txt");
        std::fs::write(&outside, "private").unwrap();
        assert!(prepare_file(
            &workspace.join("..").join("outside.txt"),
            "outside.txt",
            &workspace,
            false
        )
        .is_err());
        assert!(prepare_file(&outside, "outside.txt", &workspace, true).is_ok());
        let huge = workspace.join("huge.bin");
        let file = std::fs::File::create(&huge).unwrap();
        file.set_len(MAX_MOBILE_ATTACHMENT_BYTES + 1).unwrap();
        assert!(prepare_file(&huge, "huge.bin", &workspace, false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mobile_attachment_cannot_follow_a_workspace_symlink_to_a_private_file() {
        let root = std::env::temp_dir().join(format!("mobile-symlink-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let outside = root.join("private.txt");
        std::fs::write(&outside, b"private").unwrap();
        let link = workspace.join("report.txt");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(prepare_file(&link, "report.txt", &workspace, false).is_err());
        assert!(prepare_file(&link, "report.txt", &workspace, true).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_request_never_accepts_model_supplied_task_identity() {
        let mut valid = serde_json::json!({
            "path": std::env::temp_dir().join("report.txt"), "displayName": "report.txt",
        });
        let parsed = parse_share_request(&serde_json::to_vec(&valid).unwrap()).unwrap();
        assert_eq!(parsed.display_name, "report.txt");
        assert!(parsed.path.is_absolute());
        valid["taskId"] = serde_json::json!("forged");
        assert!(parse_share_request(&serde_json::to_vec(&valid).unwrap()).is_err());
        assert!(
            parse_share_request(br#"{"path":"../report.txt","displayName":"report.txt"}"#).is_err()
        );
    }

    #[test]
    fn mobile_attachment_stays_in_private_local_mapping_and_rejects_changed_bytes() {
        let root = std::env::temp_dir().join(format!("mobile-attachment-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        let app_data = root.join("app-data");
        std::fs::create_dir_all(&workspace).unwrap();
        let file = workspace.join("report.txt");
        std::fs::write(&file, b"test").unwrap();
        let prepared = prepare_file(&file, "report.txt", &workspace, false).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        save_record(&app_data, &id, &prepared).unwrap();
        let loaded = load_record(&app_data, &id).unwrap();
        assert_eq!(loaded.path, file.canonicalize().unwrap());
        assert!(validate_saved_file(&loaded).is_ok());
        assert!(open_verified_file(&loaded).is_ok());
        std::fs::write(&file, b"changed content").unwrap();
        assert!(validate_saved_file(&loaded).is_err());
        assert!(open_verified_file(&loaded).is_err());
        assert!(load_record(&app_data, "../bad").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_request_context_uses_one_private_capability_and_atomic_reply() {
        let root = std::env::temp_dir().join(format!("mobile-share-{}", uuid::Uuid::new_v4()));
        let (capability, directory) = create_request_context(&root).unwrap();
        assert_eq!(capability.len(), 43);
        assert!(directory.is_dir());
        assert_eq!(
            std::fs::read_to_string(root.join("mobile_share_requests/active")).unwrap(),
            capability
        );
        let request_id = uuid::Uuid::new_v4();
        std::fs::write(
            directory.join(format!("{request_id}.request.json")),
            serde_json::to_vec(&serde_json::json!({
                "path": root.join("report.txt"), "displayName": "report.txt",
            }))
            .unwrap(),
        )
        .unwrap();
        let (processing, parsed) = take_pending_request(&directory).unwrap().unwrap();
        assert_eq!(parsed.display_name, "report.txt");
        assert!(processing.to_string_lossy().ends_with(".processing.json"));
        write_response(
            &processing,
            &serde_json::json!({"attachmentId":request_id.to_string()}),
        )
        .unwrap();
        let response = directory.join(format!("{request_id}.response.json"));
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(response).unwrap()).unwrap()
                ["attachmentId"],
            request_id.to_string()
        );
        assert!(take_pending_request(&directory).unwrap().is_none());
        clear_request_context(&directory);
        assert!(!directory.exists());
        assert!(!root.join("mobile_share_requests/active").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_chunk_requires_a_grant_and_sequential_bounded_reads() {
        let root = std::env::temp_dir().join(format!("mobile-transfer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("small.txt");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let record = prepare_file(&path, "small.txt", &root, false).unwrap();
        let mut grant = TransferGrant {
            device_auth: ("desktop".into(), "token".into()),
            mobile_device_id: "phone".into(),
            file: File::open(&record.path).unwrap(),
            record,
            next_offset: 0,
            checked_at: Instant::now(),
            checked_bytes: 0,
        };
        assert_eq!(read_granted_chunk(&mut grant, 0, 4).unwrap(), b"abcd");
        assert!(read_granted_chunk(&mut grant, 0, 4).is_err());
        assert!(read_granted_chunk(&mut grant, 6, 2).is_err());
        assert!(read_granted_chunk(&mut grant, 4, TRANSFER_CHUNK_BYTES + 1).is_err());
        assert_eq!(read_granted_chunk(&mut grant, 4, 4).unwrap(), b"efgh");
        assert!(read_granted_chunk(&mut grant, 8, 1).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn mobile_attachment_rejects_and_drops_a_grant_for_a_different_desktop() {
        let root = std::env::temp_dir().join(format!("mobile-grant-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("report.txt");
        std::fs::write(&path, b"test").unwrap();
        let record = prepare_file(&path, "report.txt", &root, false).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        grants().lock().unwrap().insert(
            id.clone(),
            TransferGrant {
                // Real device credentials always have a UUID desktop identifier.
                device_auth: ("not-an-active-desktop".into(), "old-token".into()),
                mobile_device_id: "phone".into(),
                file: File::open(&record.path).unwrap(),
                record,
                next_offset: 0,
                checked_at: Instant::now(),
                checked_bytes: 0,
            },
        );
        assert_eq!(
            mobile_attachment_read_chunk(id.clone(), 0, 4)
                .await
                .unwrap_err(),
            "Desktop device changed"
        );
        assert!(!grants().lock().unwrap().contains_key(&id));
        std::fs::remove_dir_all(root).unwrap();
    }
}
