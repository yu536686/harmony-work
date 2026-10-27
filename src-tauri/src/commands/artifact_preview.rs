use crate::store::AppState;
use serde::Serialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::State;

#[path = "html_preview.rs"]
mod html_preview;

pub(crate) fn serve_html_preview(
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    html_preview::serve(request)
}

const MAX_PREVIEW_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPreviewKind {
    Image,
    Text,
    Code,
    Html,
    Pdf,
    External,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactPreviewPayload {
    pub kind: ArtifactPreviewKind,
    pub uri: String,
    pub content: Option<String>,
    pub media_type: Option<String>,
    pub truncated: bool,
    pub size_bytes: u64,
    pub preview_url: Option<String>,
    pub preview_base_url: Option<String>,
    pub is_outside_workspace: bool,
}

fn local_path_from_uri(uri: &str) -> Result<Option<PathBuf>, String> {
    let trimmed = uri.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(None);
    }

    let path = if let Some(value) = trimmed.strip_prefix("file://") {
        let decoded = urlencoding::decode(value)
            .map_err(|error| format!("成果路径编码无效：{error}"))?
            .into_owned();
        #[cfg(target_os = "windows")]
        let decoded =
            if decoded.starts_with('/') && decoded.as_bytes().get(2).copied() == Some(b':') {
                decoded[1..].to_string()
            } else {
                decoded
            };
        PathBuf::from(decoded)
    } else {
        PathBuf::from(trimmed)
    };
    Ok(Some(path))
}

fn classify_path(path: &Path) -> (ArtifactPreviewKind, Option<&'static str>) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" => (ArtifactPreviewKind::Image, Some("image/png")),
        "jpg" | "jpeg" => (ArtifactPreviewKind::Image, Some("image/jpeg")),
        "gif" => (ArtifactPreviewKind::Image, Some("image/gif")),
        "webp" => (ArtifactPreviewKind::Image, Some("image/webp")),
        "bmp" => (ArtifactPreviewKind::Image, Some("image/bmp")),
        "svg" => (ArtifactPreviewKind::Image, Some("image/svg+xml")),
        "html" | "htm" => (ArtifactPreviewKind::Html, Some("text/html")),
        "pdf" => (ArtifactPreviewKind::Pdf, Some("application/pdf")),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "css" | "scss" | "py" | "java" | "c" | "cc"
        | "cpp" | "h" | "hpp" | "go" | "sh" | "ps1" | "bat" | "sql" => {
            (ArtifactPreviewKind::Code, Some("text/plain"))
        }
        "txt" | "md" | "markdown" | "json" | "csv" | "log" | "yaml" | "yml" | "xml" | "toml"
        | "ini" => (ArtifactPreviewKind::Text, Some("text/plain")),
        _ => (ArtifactPreviewKind::External, None),
    }
}

fn is_within_workspace(file: &Path, workspace: &Path) -> bool {
    file.starts_with(workspace)
}

pub(crate) fn read_artifact_preview(
    uri: &str,
    workspace_root: &str,
    allow_outside_workspace: bool,
) -> Result<ArtifactPreviewPayload, String> {
    read_artifact_preview_with_directory(uri, workspace_root, allow_outside_workspace, false)
}

fn read_artifact_preview_with_directory(
    uri: &str,
    workspace_root: &str,
    allow_outside_workspace: bool,
    allow_directory: bool,
) -> Result<ArtifactPreviewPayload, String> {
    let Some(source_path) = local_path_from_uri(uri)? else {
        return Ok(ArtifactPreviewPayload {
            kind: ArtifactPreviewKind::External,
            uri: uri.to_string(),
            content: None,
            media_type: None,
            truncated: false,
            size_bytes: 0,
            preview_url: None,
            preview_base_url: None,
            is_outside_workspace: false,
        });
    };

    if !source_path.exists() {
        return Err(format!("成果文件不存在：{}", source_path.display()));
    }
    if !source_path.is_file() {
        return Err(format!("成果路径不是文件：{}", source_path.display()));
    }

    let canonical_file = source_path
        .canonicalize()
        .map_err(|error| format!("无法解析成果路径：{error}"))?;
    if !allow_outside_workspace {
        if workspace_root.trim().is_empty() {
            return Err("尚未配置工作区，无法安全预览成果文件。".to_string());
        }
        let canonical_workspace = Path::new(workspace_root)
            .canonicalize()
            .map_err(|error| format!("工作区路径不可用：{error}"))?;
        if !is_within_workspace(&canonical_file, &canonical_workspace) {
            return Err("该成果位于工作区之外。请将文件移入工作区，或在设置中明确启用“允许访问工作区以外的文件”。".to_string());
        }
    }

    let metadata = canonical_file
        .metadata()
        .map_err(|error| format!("无法读取成果信息：{error}"))?;
    let (kind, media_type) = classify_path(&canonical_file);
    let is_outside_workspace = Path::new(workspace_root)
        .canonicalize()
        .map(|root| !is_within_workspace(&canonical_file, &root))
        .unwrap_or(true);
    if kind == ArtifactPreviewKind::Html {
        let (content, truncated) = read_bounded_html(&canonical_file)?;
        let preview_url =
            html_preview::create(&canonical_file, workspace_root, false, allow_directory)?;
        let preview_base_url = preview_url
            .rsplit_once('/')
            .map(|(base, _)| format!("{base}/"));
        return Ok(ArtifactPreviewPayload {
            kind,
            uri: canonical_file.to_string_lossy().into_owned(),
            content: Some(content),
            media_type: media_type.map(str::to_string),
            truncated,
            size_bytes: metadata.len(),
            preview_url: Some(preview_url),
            preview_base_url,
            is_outside_workspace,
        });
    }
    if matches!(
        kind,
        ArtifactPreviewKind::Image | ArtifactPreviewKind::Pdf | ArtifactPreviewKind::External
    ) {
        return Ok(ArtifactPreviewPayload {
            kind,
            uri: canonical_file.to_string_lossy().into_owned(),
            content: None,
            media_type: media_type.map(str::to_string),
            truncated: false,
            size_bytes: metadata.len(),
            preview_url: None,
            preview_base_url: None,
            is_outside_workspace,
        });
    }

    let mut file =
        File::open(&canonical_file).map_err(|error| format!("无法打开成果文件：{error}"))?;
    let mut bytes = Vec::with_capacity(MAX_PREVIEW_BYTES + 1);
    file.by_ref()
        .take((MAX_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取成果文件：{error}"))?;
    if bytes.iter().take(8192).any(|byte| *byte == 0) {
        return Ok(ArtifactPreviewPayload {
            kind: ArtifactPreviewKind::External,
            uri: canonical_file.to_string_lossy().into_owned(),
            content: None,
            media_type: None,
            truncated: false,
            size_bytes: metadata.len(),
            preview_url: None,
            preview_base_url: None,
            is_outside_workspace,
        });
    }

    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    bytes.truncate(MAX_PREVIEW_BYTES);
    Ok(ArtifactPreviewPayload {
        kind,
        uri: canonical_file.to_string_lossy().into_owned(),
        content: Some(String::from_utf8_lossy(&bytes).into_owned()),
        media_type: media_type.map(str::to_string),
        truncated,
        size_bytes: metadata.len(),
        preview_url: None,
        preview_base_url: None,
        is_outside_workspace,
    })
}

fn read_bounded_html(path: &Path) -> Result<(String, bool), String> {
    let mut bytes = Vec::with_capacity(MAX_PREVIEW_BYTES + 1);
    File::open(path)
        .map_err(|error| format!("无法打开成果文件：{error}"))?
        .take((MAX_PREVIEW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取成果文件：{error}"))?;
    let mut truncated = bytes.len() > MAX_PREVIEW_BYTES;
    bytes.truncate(MAX_PREVIEW_BYTES);
    if truncated {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    let mut content = String::from_utf8_lossy(&bytes).into_owned();
    if content.len() > MAX_PREVIEW_BYTES {
        let mut end = MAX_PREVIEW_BYTES;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        content.truncate(end);
        truncated = true;
    }
    Ok((content, truncated))
}

fn save_artifact_copy_to(
    uri: &str,
    destination: &str,
    workspace_root: &str,
    allow_outside_workspace: bool,
) -> Result<u64, String> {
    read_artifact_preview(uri, workspace_root, allow_outside_workspace)?;
    let source = local_path_from_uri(uri)?
        .ok_or_else(|| "远程成果不能直接另存为本地文件。".to_string())?
        .canonicalize()
        .map_err(|error| format!("无法解析成果路径：{error}"))?;
    if destination.trim().is_empty() {
        return Err("请选择另存为位置。".to_string());
    }
    std::fs::copy(source, destination).map_err(|error| format!("另存成果失败：{error}"))
}

#[tauri::command]
pub async fn get_artifact_preview(
    state: State<'_, AppState>,
    session_id: String,
    uri: String,
    allow_network: Option<bool>,
    allow_directory: Option<bool>,
) -> Result<ArtifactPreviewPayload, String> {
    let settings = state.settings.lock().await.clone();
    let session = {
        let db = state.db.lock().await;
        db.get_session(&session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("会话不存在：{session_id}"))?
    };
    let workspace_root = session
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(settings.workspace_root.trim());
    let allow_outside_workspace = session
        .allow_outside_workspace
        .unwrap_or(settings.allow_outside_workspace);
    let allow_directory = allow_directory.unwrap_or(false);
    let mut preview = read_artifact_preview_with_directory(
        &uri,
        workspace_root,
        allow_outside_workspace,
        allow_directory,
    )?;
    if preview.kind == ArtifactPreviewKind::Html && allow_network.unwrap_or(false) {
        let url = html_preview::create(
            Path::new(&preview.uri),
            workspace_root,
            true,
            allow_directory,
        )?;
        preview.preview_base_url = url.rsplit_once('/').map(|(base, _)| format!("{base}/"));
        preview.preview_url = Some(url);
    }
    Ok(preview)
}

#[tauri::command]
pub async fn save_artifact_copy(
    state: State<'_, AppState>,
    session_id: String,
    uri: String,
    destination: String,
) -> Result<u64, String> {
    let settings = state.settings.lock().await.clone();
    let session = {
        let db = state.db.lock().await;
        db.get_session(&session_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("会话不存在：{session_id}"))?
    };
    let workspace_root = session
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(settings.workspace_root.trim());
    let allow_outside_workspace = session
        .allow_outside_workspace
        .unwrap_or(settings.allow_outside_workspace);
    save_artifact_copy_to(&uri, &destination, workspace_root, allow_outside_workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TestTree(PathBuf);

    impl TestTree {
        fn create() -> Self {
            let nonce = uuid::Uuid::new_v4();
            let root = std::env::temp_dir().join(format!("harmony-artifact-preview-{nonce}"));
            fs::create_dir_all(root.join("workspace")).expect("workspace");
            fs::create_dir_all(root.join("outside")).expect("outside");
            Self(root)
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn classifies_pdf_as_an_embeddable_preview() {
        let (kind, media_type) = classify_path(Path::new("C:/workspace/report.pdf"));

        assert_eq!(kind, ArtifactPreviewKind::Pdf);
        assert_eq!(media_type, Some("application/pdf"));
    }

    #[test]
    fn previews_workspace_text() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = workspace.join("result.md");
        fs::write(&file, "# result").expect("write");

        let preview = read_artifact_preview(
            file.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect("preview");

        assert_eq!(preview.kind, ArtifactPreviewKind::Text);
        assert_eq!(preview.content.as_deref(), Some("# result"));
        assert!(!preview.truncated);
    }

    #[test]
    fn html_uses_a_scoped_preview_source_instead_of_the_asset_origin() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = workspace.join("index.html");
        fs::write(&file, "<h1>Preview</h1>").unwrap();
        let preview =
            read_artifact_preview(file.to_str().unwrap(), workspace.to_str().unwrap(), false)
                .unwrap();
        let wire = serde_json::to_value(preview).unwrap();
        assert!(
            wire["preview_url"]
                .as_str()
                .is_some_and(|url| url.contains("harmony-preview")),
            "HTML must not run on the unrestricted asset origin"
        );
        assert_eq!(wire["content"], "<h1>Preview</h1>");
        assert!(wire["preview_base_url"]
            .as_str()
            .is_some_and(|url| url.ends_with('/')));
        assert_eq!(wire["is_outside_workspace"], false);
    }

    #[test]
    fn html_payload_is_bounded_utf8_and_keeps_raw_scripts_for_explicit_interaction() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = workspace.join("index.html");
        let prefix = "<script>window.ready=true</script>";
        let mut source = prefix.to_string();
        source.push_str(&"x".repeat(MAX_PREVIEW_BYTES - prefix.len() - 1));
        source.push_str("中tail");
        fs::write(&file, &source).unwrap();
        let preview =
            read_artifact_preview(file.to_str().unwrap(), workspace.to_str().unwrap(), false)
                .unwrap();
        let content = preview.content.unwrap();
        assert!(preview.truncated);
        assert_eq!(content.len(), MAX_PREVIEW_BYTES - 1);
        assert!(content.starts_with(prefix));
        assert!(!content.contains('\u{fffd}'));
        assert_eq!(preview.size_bytes, source.len() as u64);

        fs::write(&file, [0xff; MAX_PREVIEW_BYTES + 1]).unwrap();
        let preview =
            read_artifact_preview(file.to_str().unwrap(), workspace.to_str().unwrap(), false)
                .unwrap();
        let content = preview.content.unwrap();
        assert!(preview.truncated);
        assert!(content.len() <= MAX_PREVIEW_BYTES);
        assert!(std::str::from_utf8(content.as_bytes()).is_ok());
    }

    #[test]
    fn outside_file_permission_is_not_a_directory_resource_grant() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let outside = tree.0.join("outside");
        let file = outside.join("index.html");
        fs::write(&file, "<img src=photo.png><script src=app.js></script>").unwrap();
        fs::write(outside.join("private.txt"), "synthetic sentinel").unwrap();
        for allow_directory in [false, true] {
            let preview = read_artifact_preview_with_directory(
                file.to_str().unwrap(),
                workspace.to_str().unwrap(),
                true,
                allow_directory,
            )
            .unwrap();
            assert!(preview.is_outside_workspace);
            assert!(preview.content.unwrap().contains("<script"));
            let base: tauri::http::Uri = preview.preview_base_url.unwrap().parse().unwrap();
            let response = serve_html_preview(
                tauri::http::Request::builder()
                    .uri(format!("{}private.txt", base.path()))
                    .body(Vec::new())
                    .unwrap(),
            );
            assert_eq!(response.status(), if allow_directory { 200 } else { 403 });
        }
        assert!(read_artifact_preview_with_directory(
            file.to_str().unwrap(),
            workspace.to_str().unwrap(),
            false,
            true
        )
        .is_err());
    }

    #[test]
    fn blocks_files_outside_workspace_until_authorized() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = tree.0.join("outside/result.txt");
        fs::write(&file, "outside").expect("write");

        let denied = read_artifact_preview(
            file.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect_err("must deny");
        assert!(denied.contains("工作区之外"));

        let allowed = read_artifact_preview(
            file.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            true,
        )
        .expect("allowed");
        assert_eq!(allowed.content.as_deref(), Some("outside"));
    }

    #[test]
    fn truncates_large_text_preview() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = workspace.join("large.txt");
        fs::write(&file, vec![b'a'; MAX_PREVIEW_BYTES + 32]).expect("write");

        let preview = read_artifact_preview(
            file.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect("preview");
        assert!(preview.truncated);
        assert_eq!(preview.content.expect("content").len(), MAX_PREVIEW_BYTES);
    }

    #[test]
    fn treats_binary_content_as_external() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let file = workspace.join("unknown.txt");
        fs::write(&file, [0, 1, 2, 3]).expect("write");

        let preview = read_artifact_preview(
            file.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect("preview");
        assert_eq!(preview.kind, ArtifactPreviewKind::External);
        assert!(preview.content.is_none());
    }

    #[test]
    fn missing_file_error_is_actionable() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let missing = workspace.join("missing.txt");
        let error = read_artifact_preview(
            missing.to_str().expect("path"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect_err("missing");
        assert!(error.contains("成果文件不存在"));
    }

    #[test]
    fn validated_artifact_can_be_copied_to_user_destination() {
        let tree = TestTree::create();
        let workspace = tree.0.join("workspace");
        let source = workspace.join("report.docx");
        let destination = tree.0.join("report-copy.docx");
        fs::write(&source, b"office payload").expect("write");
        save_artifact_copy_to(
            source.to_str().expect("path"),
            destination.to_str().expect("destination"),
            workspace.to_str().expect("workspace"),
            false,
        )
        .expect("copy");
        assert_eq!(fs::read(destination).expect("read"), b"office payload");
    }
}
