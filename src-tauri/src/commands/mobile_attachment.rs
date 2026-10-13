//! Local file registration for mobile conversations. Paths never leave this desktop.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Manager};

pub(crate) const MAX_MOBILE_ATTACHMENT_BYTES: u64 = 200 * 1024 * 1024;

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
    if raw.len() > 8 * 1024 { return Err("File share request is too large".into()); }
    let request: ShareRequest = serde_json::from_slice(raw)
        .map_err(|_| "Invalid file share request".to_string())?;
    if !request.path.is_absolute() || request.path.as_os_str().len() > 4096 {
        return Err("File share path must be absolute".into());
    }
    let name = request.display_name.trim();
    if name.is_empty() || name.chars().count() > 120 ||
        name.chars().any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control()) {
        return Err("Invalid file share display name".into());
    }
    Ok(request)
}

pub(crate) fn save_record(
    app_data_dir: &Path, attachment_id: &str, record: &PreparedAttachment,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    let directory = app_data_dir.join("mobile_attachments");
    std::fs::create_dir_all(&directory).map_err(|_| "Cannot create local attachment store".to_string())?;
    let destination = directory.join(format!("{id}.json"));
    let temporary = directory.join(format!("{id}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, serde_json::to_vec(record).map_err(|_| "Cannot encode attachment".to_string())?)
        .map_err(|_| "Cannot save local attachment".to_string())?;
    if let Err(error) = std::fs::rename(&temporary, &destination) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("Cannot commit local attachment: {error}"));
    }
    Ok(())
}

pub(crate) fn load_record(app_data_dir: &Path, attachment_id: &str) -> Result<PreparedAttachment, String> {
    let id = uuid::Uuid::parse_str(attachment_id).map_err(|_| "Invalid attachment ID".to_string())?;
    let path = app_data_dir.join("mobile_attachments").join(format!("{id}.json"));
    let metadata = path.metadata().map_err(|_| "Local attachment not found".to_string())?;
    if metadata.len() > 16 * 1024 { return Err("Invalid local attachment record".into()); }
    let record: PreparedAttachment = serde_json::from_slice(
        &std::fs::read(path).map_err(|_| "Cannot read local attachment".to_string())?
    ).map_err(|_| "Invalid local attachment record".to_string())?;
    if !record.path.is_absolute() || record.size > MAX_MOBILE_ATTACHMENT_BYTES ||
        record.sha256.len() != 64 || !record.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid local attachment record".into());
    }
    Ok(record)
}

pub(crate) fn validate_saved_file(record: &PreparedAttachment) -> Result<(), String> {
    let current = prepare_file(&record.path, &record.name, Path::new(""), true)?;
    if current.path != record.path || current.size != record.size ||
        current.modified_ns != record.modified_ns || current.sha256 != record.sha256 {
        return Err("Attachment file changed or moved".into());
    }
    Ok(())
}

pub(crate) fn create_request_context(app_data_dir: &Path) -> Result<(String, PathBuf), String> {
    let parent = app_data_dir.join("mobile_share_requests");
    std::fs::create_dir_all(&parent).map_err(|_| "Cannot create mobile task request store".to_string())?;
    for _ in 0..4 {
        let mut random = [0_u8; 32];
        getrandom::getrandom(&mut random).map_err(|_| "Cannot create mobile task capability".to_string())?;
        let capability = URL_SAFE_NO_PAD.encode(random);
        let directory = parent.join(&capability);
        if std::fs::create_dir(&directory).is_ok() { return Ok((capability, directory)); }
    }
    Err("Cannot allocate mobile task request store".into())
}

pub(crate) fn take_pending_request(directory: &Path) -> Result<Option<(PathBuf, ShareRequest)>, String> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|_| "Mobile task request store is unavailable".to_string())?
        .flatten().map(|entry| entry.path()).collect::<Vec<_>>();
    entries.sort();
    for request_path in entries {
        let Some(name) = request_path.file_name().and_then(|value| value.to_str()) else { continue; };
        let Some(id) = name.strip_suffix(".request.json") else { continue; };
        if uuid::Uuid::parse_str(id).is_err() { continue; }
        let processing = directory.join(format!("{id}.processing.json"));
        if std::fs::rename(&request_path, &processing).is_err() { continue; }
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
    let name = processing.file_name().and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid mobile file request".to_string())?;
    let id = name.strip_suffix(".processing.json")
        .ok_or_else(|| "Invalid mobile file request".to_string())?;
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid mobile file request".to_string())?;
    let directory = processing.parent().ok_or_else(|| "Invalid mobile file request".to_string())?;
    let temporary = directory.join(format!("{id}.reply.tmp"));
    let destination = directory.join(format!("{id}.response.json"));
    std::fs::write(&temporary, serde_json::to_vec(response).map_err(|_| "Cannot encode file share result".to_string())?)
        .map_err(|_| "Cannot write file share result".to_string())?;
    std::fs::rename(&temporary, &destination)
        .map_err(|_| "Cannot publish file share result".to_string())?;
    let _ = std::fs::remove_file(processing);
    Ok(())
}

#[tauri::command]
pub async fn mobile_attachment_open(app: AppHandle, attachment_id: String) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|_| "Attachment store unavailable".to_string())?;
    let path = tokio::task::spawn_blocking(move || {
        let record = load_record(&app_data_dir, &attachment_id)?;
        validate_saved_file(&record)?;
        Ok::<PathBuf, String>(record.path)
    }).await.map_err(|_| "Attachment inspection stopped unexpectedly".to_string())??;
    crate::app::bootstrap::open_path(path.to_string_lossy().into_owned())
}

pub(crate) fn prepare_file(
    path: &Path, display_name: &str, workspace_root: &Path,
    allow_outside_workspace: bool,
) -> Result<PreparedAttachment, String> {
    let name = display_name.trim();
    if name.is_empty() || name.chars().count() > 120 || name == "." || name == ".." ||
        name.chars().any(|ch| ch == '/' || ch == '\\' || ch == ':' || ch.is_control()) {
        return Err("Invalid attachment display name".into());
    }
    let canonical = path.canonicalize().map_err(|_| "File not found".to_string())?;
    if !allow_outside_workspace {
        let root = workspace_root.canonicalize()
            .map_err(|_| "Workspace is unavailable".to_string())?;
        if !canonical.starts_with(root) {
            return Err("File is outside the authorized workspace".into());
        }
    }
    let metadata = canonical.metadata().map_err(|_| "Cannot inspect file".to_string())?;
    if !metadata.is_file() { return Err("Only regular files can be shared".into()); }
    if metadata.len() > MAX_MOBILE_ATTACHMENT_BYTES {
        return Err("File exceeds the 200 MiB mobile transfer limit".into());
    }
    let modified_ns = metadata.modified().map_err(|_| "Cannot inspect file timestamp".to_string())?
        .duration_since(UNIX_EPOCH).map_err(|_| "Invalid file timestamp".to_string())?.as_nanos();
    let mut file = File::open(&canonical).map_err(|_| "Cannot open file".to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| "Cannot read file".to_string())?;
        if read == 0 { break; }
        digest.update(&buffer[..read]);
    }
    let after = canonical.metadata().map_err(|_| "File changed during registration".to_string())?;
    let after_ns = after.modified().map_err(|_| "File changed during registration".to_string())?
        .duration_since(UNIX_EPOCH).map_err(|_| "File changed during registration".to_string())?.as_nanos();
    if after.len() != metadata.len() || after_ns != modified_ns {
        return Err("File changed during registration".into());
    }
    let mime_type = match canonical.extension().and_then(|value| value.to_str())
        .unwrap_or("").to_ascii_lowercase().as_str() {
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
        }.to_string();
    Ok(PreparedAttachment {
        path: canonical, name: name.to_string(), mime_type,
        size: metadata.len(), sha256: hex::encode(digest.finalize()), modified_ns,
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
        assert_eq!(prepared.sha256, "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_rejects_parent_traversal_and_oversize_files() {
        let root = std::env::temp_dir().join(format!("mobile-attachment-{}", uuid::Uuid::new_v4()));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let outside = root.join("outside.txt");
        std::fs::write(&outside, "private").unwrap();
        assert!(prepare_file(&workspace.join("..").join("outside.txt"), "outside.txt", &workspace, false).is_err());
        assert!(prepare_file(&outside, "outside.txt", &workspace, true).is_ok());
        let huge = workspace.join("huge.bin");
        let file = std::fs::File::create(&huge).unwrap();
        file.set_len(MAX_MOBILE_ATTACHMENT_BYTES + 1).unwrap();
        assert!(prepare_file(&huge, "huge.bin", &workspace, false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_request_never_accepts_model_supplied_task_identity() {
        let valid = br#"{"path":"C:\\Users\\owner\\report.txt","displayName":"report.txt"}"#;
        let parsed = parse_share_request(valid).unwrap();
        assert_eq!(parsed.display_name, "report.txt");
        assert!(parsed.path.is_absolute());
        let forged = br#"{"path":"C:\\Users\\owner\\report.txt","displayName":"report.txt","taskId":"forged"}"#;
        assert!(parse_share_request(forged).is_err());
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
        std::fs::write(&file, b"changed content").unwrap();
        assert!(validate_saved_file(&loaded).is_err());
        assert!(load_record(&app_data, "../bad").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mobile_attachment_request_context_uses_one_private_capability_and_atomic_reply() {
        let root = std::env::temp_dir().join(format!("mobile-share-{}", uuid::Uuid::new_v4()));
        let (capability, directory) = create_request_context(&root).unwrap();
        assert_eq!(capability.len(), 43);
        assert!(directory.is_dir());
        let request_id = uuid::Uuid::new_v4();
        std::fs::write(directory.join(format!("{request_id}.request.json")),
            r#"{"path":"C:\\Users\\owner\\report.txt","displayName":"report.txt"}"#).unwrap();
        let (processing, parsed) = take_pending_request(&directory).unwrap().unwrap();
        assert_eq!(parsed.display_name, "report.txt");
        assert!(processing.to_string_lossy().ends_with(".processing.json"));
        write_response(&processing, &serde_json::json!({"attachmentId":request_id.to_string()})).unwrap();
        let response = directory.join(format!("{request_id}.response.json"));
        assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(response).unwrap()).unwrap()["attachmentId"], request_id.to_string());
        assert!(take_pending_request(&directory).unwrap().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
