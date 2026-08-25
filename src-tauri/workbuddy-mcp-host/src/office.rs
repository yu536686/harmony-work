use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const ARTIFACT_TIMEOUT_SECS: u64 = 180;
const EMBEDDED_RUNTIME: &[u8] =
    include_bytes!("../../resources/artifact-runtime/artifact-runtime.cjs");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Document,
    Presentation,
    Spreadsheet,
}

impl ArtifactKind {
    fn from_tool(name: &str) -> Option<Self> {
        match name {
            "document_artifact" => Some(Self::Document),
            "presentation_artifact" => Some(Self::Presentation),
            "spreadsheet_artifact" => Some(Self::Spreadsheet),
            _ => None,
        }
    }

    fn runtime_name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Presentation => "presentation",
            Self::Spreadsheet => "spreadsheet",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Document => "docx",
            Self::Presentation => "pptx",
            Self::Spreadsheet => "xlsx",
        }
    }

    fn artifact_type(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Presentation => "presentation",
            Self::Spreadsheet => "spreadsheet",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeOutput {
    ok: bool,
    path: String,
    bytes: u64,
    #[serde(default)]
    details: Value,
}

pub(crate) fn handles(name: &str) -> bool {
    ArtifactKind::from_tool(name).is_some()
}

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"document_artifact",
            "description":"Create a verified DOCX directly from structured sections using WorkBuddy Artifact Runtime. This is the default tool for new Word documents; do not generate a Python script or use Word COM.",
            "inputSchema":{
                "type":"object",
                "properties":{
                    "workspace_root":{"type":"string","description":"Absolute active workspace path."},
                    "path":{"type":"string","description":"Workspace-relative or absolute .docx path inside workspace_root."},
                    "title":{"type":"string"},"subtitle":{"type":"string"},"author":{"type":"string"},
                    "header_text":{"type":"string"},"footer_text":{"type":"string"},
                    "append":{"type":"boolean","description":"Append sections to the last successful draft for this path."},
                    "sections":{"type":"array","maxItems":80,"items":{"type":"object","properties":{
                        "heading":{"type":"string"},"level":{"type":"integer","enum":[1,2,3]},
                        "paragraphs":{"type":"array","items":{"type":"string"}},
                        "bullets":{"type":"array","items":{"type":"string"}},
                        "table":{"type":"object"},"image_path":{"type":"string"},
                        "image_width":{"type":"integer"},"image_height":{"type":"integer"},
                        "page_break":{"type":"boolean"}
                    }}}
                },
                "required":["workspace_root","path","sections"],"additionalProperties":true
            },
            "annotations":{"title":"创建 Word 文档","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false}
        }),
        json!({
            "name":"presentation_artifact",
            "description":"Create a polished verified PPTX from structured slides using WorkBuddy Artifact Runtime. This is the default tool for new presentations; do not generate a Python script or use PowerPoint COM.",
            "inputSchema":{
                "type":"object","properties":{
                    "workspace_root":{"type":"string","description":"Absolute active workspace path."},
                    "path":{"type":"string","description":"Workspace-relative or absolute .pptx path inside workspace_root."},
                    "title":{"type":"string"},"subtitle":{"type":"string"},"author":{"type":"string"},
                    "theme":{"type":"string","enum":["tech_dark","clean_light","business_blue","editorial"]},
                    "slides":{"type":"array","minItems":1,"maxItems":40,"items":{"type":"object","properties":{
                        "title":{"type":"string"},"subtitle":{"type":"string"},"body":{"type":"string"},
                        "bullets":{"type":"array","items":{"type":"string"}},
                        "layout":{"type":"string","enum":["section","bullets","two_column","quote","image","summary"]},
                        "image_path":{"type":"string"},"accent":{"type":"string"},
                        "left":{"type":"object"},"right":{"type":"object"}
                    },"required":["title"]}}
                },
                "required":["workspace_root","path","title","slides"],"additionalProperties":true
            },
            "annotations":{"title":"创建 PowerPoint 演示文稿","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false}
        }),
        json!({
            "name":"spreadsheet_artifact",
            "description":"Create a verified XLSX workbook from structured sheets using WorkBuddy Artifact Runtime. This is the default tool for new spreadsheets; do not generate a Python script or use Excel COM.",
            "inputSchema":{
                "type":"object","properties":{
                    "workspace_root":{"type":"string","description":"Absolute active workspace path."},
                    "path":{"type":"string","description":"Workspace-relative or absolute .xlsx path inside workspace_root."},
                    "creator":{"type":"string"},
                    "sheets":{"type":"array","minItems":1,"maxItems":30,"items":{"type":"object","properties":{
                        "name":{"type":"string","minLength":1,"maxLength":31},"columns":{"type":"array"},
                        "rows":{"type":"array"},"cells":{"type":"array"},"freeze_header":{"type":"boolean"},
                        "auto_filter":{"type":"boolean"}
                    },"required":["name"]}}
                },
                "required":["workspace_root","path","sheets"],"additionalProperties":true
            },
            "annotations":{"title":"创建 Excel 工作簿","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false}
        }),
    ]
}

pub(crate) async fn call(name: &str, arguments: &Value) -> Result<Value> {
    let kind = ArtifactKind::from_tool(name).ok_or_else(|| anyhow!("unknown office tool"))?;
    run_artifact_runtime(kind, arguments.clone()).await
}

async fn run_artifact_runtime(kind: ArtifactKind, mut input: Value) -> Result<Value> {
    let workspace_text = required_string(&input, "workspace_root")?;
    let workspace_root = PathBuf::from(&workspace_text);
    if !workspace_root.is_absolute() {
        return Err(anyhow!("workspace_root must be an absolute path"));
    }
    std::fs::create_dir_all(&workspace_root)?;
    let requested_path = required_string(&input, "path")?;
    validate_extension(kind, &requested_path)?;
    let output_path = safe_output_path(&workspace_root, &requested_path)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    input
        .as_object_mut()
        .ok_or_else(|| anyhow!("artifact input must be an object"))?
        .remove("workspace_root");
    input["path"] = Value::String(output_path.to_string_lossy().to_string());
    normalize_embedded_asset_paths(kind, &mut input, &workspace_root);

    let state_path = if kind == ArtifactKind::Document {
        let state = document_state_path(&workspace_root, &output_path);
        prepare_document_input(&mut input, &state)?;
        Some(state)
    } else {
        None
    };
    let runtime = write_embedded_runtime(&workspace_root)?;
    let jobs = workspace_root
        .join(".workbuddy")
        .join("artifact-runtime")
        .join("jobs");
    std::fs::create_dir_all(&jobs)?;
    let spec_path = jobs.join(format!("{}.json", uuid::Uuid::new_v4().simple()));
    std::fs::write(&spec_path, serde_json::to_vec(&input)?)?;

    let mut command = Command::new(find_node()?);
    command
        .arg(runtime)
        .arg(kind.runtime_name())
        .arg(&spec_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = match timeout(Duration::from_secs(ARTIFACT_TIMEOUT_SECS), command.output()).await {
        Ok(result) => result.context("Failed to start Artifact Runtime")?,
        Err(_) => {
            let _ = std::fs::remove_file(&spec_path);
            return Err(anyhow!(
                "Artifact Runtime timed out after {ARTIFACT_TIMEOUT_SECS} seconds"
            ));
        }
    };
    let _ = std::fs::remove_file(&spec_path);
    let stdout = trim_process_output(&output.stdout);
    let stderr = trim_process_output(&output.stderr);
    if !output.status.success() {
        return Err(anyhow!(
            "Artifact Runtime failed (status={}): {}",
            output.status,
            if stderr.is_empty() { stdout } else { stderr }
        ));
    }
    let parsed = parse_runtime_output(&stdout, &output_path)?;
    let metadata = std::fs::metadata(&output_path)
        .with_context(|| format!("Artifact Runtime did not create {}", output_path.display()))?;
    if !metadata.is_file() || metadata.len() < 512 {
        return Err(anyhow!("Artifact Runtime output is unexpectedly small"));
    }
    if let Some(state_path) = state_path.as_ref() {
        persist_document_state(state_path, &input)?;
    }
    let name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .to_string();
    let path = output_path.to_string_lossy().to_string();
    let payload = json!({
        "ok":true,"kind":kind.runtime_name(),"path":path,"bytes":metadata.len(),
        "details":parsed.details,
        "artifacts":[{"name":name,"artifactType":kind.artifact_type(),"uri":path}]
    });
    Ok(json!({
        "content":[
            {"type":"text","text":format!("Created {}: {} ({} bytes)", kind.artifact_type(), output_path.display(), metadata.len())},
            {"type":"resource_link","name":name,"uri":file_uri(&output_path),"mimeType":mime_type(kind),"size":metadata.len()}
        ],
        "structuredContent":payload,"isError":false
    }))
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn validate_extension(kind: ArtifactKind, path: &str) -> Result<()> {
    let extension = Path::new(path)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case(kind.extension()) {
        Ok(())
    } else {
        Err(anyhow!(
            "{} path must end with .{}",
            kind.runtime_name(),
            kind.extension()
        ))
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn safe_output_path(workspace_root: &Path, requested: &str) -> Result<PathBuf> {
    let workspace = lexical_normalize(workspace_root);
    let requested = Path::new(requested);
    let output = lexical_normalize(
        if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            workspace.join(requested)
        }
        .as_path(),
    );
    if !output.starts_with(&workspace) {
        return Err(anyhow!("artifact path must stay inside workspace_root"));
    }
    Ok(output)
}

fn normalize_embedded_asset_paths(kind: ArtifactKind, input: &mut Value, workspace_root: &Path) {
    let collection = match kind {
        ArtifactKind::Document => "sections",
        ArtifactKind::Presentation => "slides",
        ArtifactKind::Spreadsheet => return,
    };
    let Some(items) = input.get_mut(collection).and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(path) = item
            .get("image_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        else {
            continue;
        };
        let path = Path::new(path);
        if !path.is_absolute() {
            item["image_path"] =
                Value::String(workspace_root.join(path).to_string_lossy().to_string());
        }
    }
}

fn document_state_path(workspace_root: &Path, output_path: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    output_path.to_string_lossy().hash(&mut hasher);
    workspace_root
        .join(".workbuddy")
        .join("artifact-runtime")
        .join("drafts")
        .join(format!("document-{:016x}.json", hasher.finish()))
}

fn prepare_document_input(input: &mut Value, state_path: &Path) -> Result<()> {
    let append = input
        .get("append")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    input
        .as_object_mut()
        .ok_or_else(|| anyhow!("document input must be an object"))?
        .remove("append");
    if append {
        let raw = std::fs::read_to_string(state_path).with_context(|| {
            format!(
                "No document draft exists for append mode: {}",
                state_path.display()
            )
        })?;
        let mut previous: Value =
            serde_json::from_str(&raw).context("Document draft state is invalid")?;
        let additions = input
            .get("sections")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| anyhow!("sections must be an array"))?;
        previous
            .get_mut("sections")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Persisted document sections are invalid"))?
            .extend(additions);
        *input = previous;
    }
    let count = input
        .get("sections")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| anyhow!("sections must be an array"))?;
    if count > 80 {
        return Err(anyhow!("document supports at most 80 sections"));
    }
    Ok(())
}

fn persist_document_state(path: &Path, input: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(input)?)?;
    Ok(())
}

fn write_embedded_runtime(workspace_root: &Path) -> Result<PathBuf> {
    let directory = workspace_root.join(".workbuddy").join("artifact-runtime");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("artifact-runtime-1.0.0-beta.1.cjs");
    if std::fs::metadata(&path)
        .map(|m| m.len() != EMBEDDED_RUNTIME.len() as u64)
        .unwrap_or(true)
    {
        std::fs::write(&path, EMBEDDED_RUNTIME)?;
    }
    Ok(path)
}

fn node_candidates() -> Vec<OsString> {
    let mut values = ["WORKBUDDY_NODE", "NODE"]
        .iter()
        .filter_map(|key| std::env::var_os(key).filter(|v| !v.is_empty()))
        .collect::<Vec<_>>();
    if cfg!(target_os = "windows") {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                let path = if key == "LOCALAPPDATA" {
                    root.join("Programs/nodejs/node.exe")
                } else {
                    root.join("nodejs/node.exe")
                };
                values.push(path.into_os_string());
            }
        }
        values.extend([OsString::from("node.exe"), OsString::from("node")]);
    } else {
        values.extend([
            OsString::from("/opt/homebrew/bin/node"),
            OsString::from("/usr/local/bin/node"),
            OsString::from("/usr/bin/node"),
            OsString::from("node"),
        ]);
    }
    values
}

fn find_node() -> Result<OsString> {
    node_candidates().into_iter().find(|candidate| {
        std::process::Command::new(candidate).arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    }).ok_or_else(|| anyhow!("Node.js 18 or newer was not found. Open the WorkBuddy initializer and install Node.js, then retry."))
}

fn parse_runtime_output(stdout: &str, expected_path: &Path) -> Result<RuntimeOutput> {
    let parsed: RuntimeOutput =
        serde_json::from_str(stdout.trim()).context("Artifact Runtime returned invalid JSON")?;
    if !parsed.ok
        || lexical_normalize(Path::new(&parsed.path)) != lexical_normalize(expected_path)
        || parsed.bytes < 512
    {
        return Err(anyhow!(
            "Artifact Runtime returned an invalid output result"
        ));
    }
    Ok(parsed)
}

fn trim_process_output(bytes: &[u8]) -> String {
    let mut value = String::from_utf8_lossy(bytes).trim().to_string();
    if value.chars().count() > 4_000 {
        value = value.chars().take(4_000).collect::<String>();
        value.push_str("\n...[truncated]");
    }
    value
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.to_string_lossy().replace('\\', "/"))
}

fn mime_type(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Document => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        }
        ArtifactKind::Presentation => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        ArtifactKind::Spreadsheet => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_output_outside_workspace() {
        let root = std::env::temp_dir().join("workbuddy-office-root");
        assert!(safe_output_path(&root, "../escape.docx").is_err());
        assert_eq!(
            safe_output_path(&root, "reports/report.docx").unwrap(),
            root.join("reports/report.docx")
        );
    }
}
