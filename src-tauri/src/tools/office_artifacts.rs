use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use piscis_kernel::agent::tool::{
    Tool, ToolContext, ToolExecutionMode, ToolExposure, ToolMetadata, ToolResult, ToolRisk,
};
use piscis_kernel::tools::output::ToolArtifact;
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const ARTIFACT_TIMEOUT_SECS: u64 = 180;
const RENDER_TIMEOUT_SECS: u64 = 180;
const EMBEDDED_RUNTIME: &[u8] =
    include_bytes!("../../resources/artifact-runtime/artifact-runtime.cjs");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactKind {
    Document,
    Presentation,
    Spreadsheet,
}

impl ArtifactKind {
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

fn validate_artifact_path(kind: ArtifactKind, path: &str) -> Result<()> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case(kind.extension()) {
        return Err(anyhow!(
            "{} path must end with .{}",
            kind.runtime_name(),
            kind.extension()
        ));
    }
    Ok(())
}

fn normalized_path(path: &str) -> String {
    let value = path.replace('\\', "/");
    if cfg!(target_os = "windows") {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn parse_runtime_output(stdout: &str, expected_path: &str) -> Result<RuntimeOutput> {
    let parsed: RuntimeOutput =
        serde_json::from_str(stdout.trim()).context("Artifact Runtime returned invalid JSON")?;
    if !parsed.ok {
        return Err(anyhow!("Artifact Runtime reported an unsuccessful result"));
    }
    if normalized_path(&parsed.path) != normalized_path(expected_path) {
        return Err(anyhow!(
            "Artifact Runtime returned an unexpected output path: {}",
            parsed.path
        ));
    }
    if parsed.bytes < 512 {
        return Err(anyhow!("Artifact Runtime output is unexpectedly small"));
    }
    Ok(parsed)
}

fn resolve_path(path: &str, workspace_root: &Path) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn normalize_embedded_asset_paths(
    kind: ArtifactKind,
    input: &mut Value,
    workspace_root: &Path,
) {
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
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        item["image_path"] = Value::String(
            resolve_path(path, workspace_root)
                .to_string_lossy()
                .to_string(),
        );
    }
}

fn document_state_path(workspace_root: &Path, output_path: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    normalized_path(&output_path.to_string_lossy()).hash(&mut hasher);
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
                "No document draft exists for append mode: {}. Create the first section batch without append=true.",
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
        let sections = previous
            .get_mut("sections")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Persisted document sections are invalid"))?;
        sections.extend(additions);
        *input = previous;
    }

    let section_count = input
        .get("sections")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| anyhow!("sections must be an array"))?;
    if section_count > 80 {
        return Err(anyhow!("document supports at most 80 sections"));
    }
    Ok(())
}

fn persist_document_state(state_path: &Path, input: &Value) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(state_path, serde_json::to_vec(input)?)?;
    Ok(())
}

fn write_embedded_runtime(workspace_root: &Path) -> Result<PathBuf> {
    let directory = workspace_root.join(".workbuddy").join("artifact-runtime");
    std::fs::create_dir_all(&directory)?;
    let path = directory.join("artifact-runtime-0.9.4.cjs");
    let needs_write = std::fs::metadata(&path)
        .map(|metadata| metadata.len() != EMBEDDED_RUNTIME.len() as u64)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, EMBEDDED_RUNTIME)?;
    }
    Ok(path)
}

fn command_available(command: &OsString, version_arg: &str) -> bool {
    std::process::Command::new(command)
        .arg(version_arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn node_candidates() -> Vec<OsString> {
    let mut candidates = Vec::new();
    for key in ["WORKBUDDY_NODE", "NODE"] {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            candidates.push(value);
        }
    }
    if cfg!(target_os = "windows") {
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(key) {
                let root = PathBuf::from(root);
                let path = if key == "LOCALAPPDATA" {
                    root.join("Programs").join("nodejs").join("node.exe")
                } else {
                    root.join("nodejs").join("node.exe")
                };
                candidates.push(path.into_os_string());
            }
        }
        candidates.push(OsString::from("node.exe"));
        candidates.push(OsString::from("node"));
    } else {
        candidates.extend([
            OsString::from("/opt/homebrew/bin/node"),
            OsString::from("/usr/local/bin/node"),
            OsString::from("/usr/bin/node"),
            OsString::from("node"),
        ]);
    }
    candidates
}

fn find_node() -> Result<OsString> {
    node_candidates()
        .into_iter()
        .find(|candidate| command_available(candidate, "--version"))
        .ok_or_else(|| {
            anyhow!(
                "Node.js 18 or newer was not found. Open the WorkBuddy initializer and install Node.js, then retry."
            )
        })
}

fn trim_process_output(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut output = text.trim().to_string();
    if output.chars().count() > 4_000 {
        output = output.chars().take(4_000).collect::<String>();
        output.push_str("\n...[truncated]");
    }
    output
}

async fn run_artifact_runtime(
    kind: ArtifactKind,
    mut input: Value,
    ctx: &ToolContext,
) -> Result<ToolResult> {
    let path = input
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("path is required"))?;
    validate_artifact_path(kind, path)?;
    let output_path = resolve_path(path, &ctx.workspace_root);
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let output_path_text = output_path.to_string_lossy().to_string();
    input["path"] = Value::String(output_path_text.clone());
    normalize_embedded_asset_paths(kind, &mut input, &ctx.workspace_root);
    let document_state = if kind == ArtifactKind::Document {
        let state = document_state_path(&ctx.workspace_root, &output_path);
        prepare_document_input(&mut input, &state)?;
        Some(state)
    } else {
        None
    };

    let runtime = write_embedded_runtime(&ctx.workspace_root)?;
    let jobs = ctx
        .workspace_root
        .join(".workbuddy")
        .join("artifact-runtime")
        .join("jobs");
    std::fs::create_dir_all(&jobs)?;
    let spec_path = jobs.join(format!("{}.json", uuid::Uuid::new_v4().simple()));
    std::fs::write(&spec_path, serde_json::to_vec(&input)?)?;

    let node = find_node()?;
    let mut command = Command::new(node);
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
            return Ok(ToolResult::err(format!(
                "Artifact Runtime timed out after {ARTIFACT_TIMEOUT_SECS} seconds"
            )));
        }
    };
    let _ = std::fs::remove_file(&spec_path);
    let stdout = trim_process_output(&output.stdout);
    let stderr = trim_process_output(&output.stderr);
    if !output.status.success() {
        return Ok(ToolResult::err(format!(
            "Artifact Runtime failed (status={}): {}",
            output.status,
            if stderr.is_empty() { stdout } else { stderr }
        )));
    }
    let parsed = match parse_runtime_output(&stdout, &output_path_text) {
        Ok(parsed) => parsed,
        Err(error) => return Ok(ToolResult::err(error.to_string())),
    };
    let metadata = match std::fs::metadata(&output_path) {
        Ok(metadata) if metadata.is_file() && metadata.len() >= 512 => metadata,
        _ => {
            return Ok(ToolResult::err(format!(
                "Artifact Runtime did not create a valid output file: {}",
                output_path.display()
            )))
        }
    };
    if let Some(state_path) = document_state.as_ref() {
        persist_document_state(state_path, &input)?;
    }
    let name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .to_string();
    let payload = json!({
        "ok": true,
        "kind": kind.runtime_name(),
        "path": output_path_text,
        "bytes": metadata.len(),
        "details": parsed.details,
    });
    Ok(ToolResult::ok(format!(
        "Created {}: {} ({} bytes)",
        kind.artifact_type(),
        output_path.display(),
        metadata.len()
    ))
    .with_data(payload.clone())
    .with_artifacts(vec![ToolArtifact {
        name,
        artifact_type: kind.artifact_type().to_string(),
        uri: Some(output_path.to_string_lossy().to_string()),
        content_summary: format!(
            "{} generated and OOXML package verified",
            kind.artifact_type()
        ),
        metadata: Some(payload),
    }]))
}

fn artifact_metadata() -> ToolMetadata {
    ToolMetadata {
        category: "office_artifact",
        exposure: ToolExposure::Deferred,
        execution: ToolExecutionMode::Exclusive,
        risk: ToolRisk::Write,
        timeout_secs: ARTIFACT_TIMEOUT_SECS,
        produces_artifacts: true,
    }
}

pub struct DocumentArtifactTool;

#[async_trait]
impl Tool for DocumentArtifactTool {
    fn name(&self) -> &str {
        "document_artifact"
    }
    fn description(&self) -> &str {
        "Create a new verified DOCX from structured sections. Prefer this for new Word deliverables. Supports paragraphs, bullets, tables, local images, headers, footers, and page breaks without Microsoft Word or COM. For a long report, create the first section batch normally, then call the same tool with append=true and smaller section batches."
    }
    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed(
            "Create a new verified DOCX from structured sections; default for new Word files.",
        )
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute or workspace-relative .docx output path." },
                "title": { "type": "string" },
                "subtitle": { "type": "string" },
                "author": { "type": "string" },
                "header_text": { "type": "string" },
                "footer_text": { "type": "string" },
                "append": { "type": "boolean", "description": "Append these sections to this path's last successful document draft. Use smaller batches for long reports." },
                "sections": {
                    "type": "array",
                    "maxItems": 80,
                    "items": {
                        "type": "object",
                        "properties": {
                            "heading": { "type": "string" },
                            "level": { "type": "integer", "enum": [1, 2, 3] },
                            "paragraphs": { "type": "array", "items": { "type": "string" } },
                            "bullets": { "type": "array", "items": { "type": "string" } },
                            "table": {
                                "type": "object",
                                "properties": {
                                    "headers": { "type": "array", "items": { "type": ["string", "number"] } },
                                    "rows": { "type": "array", "items": { "type": "array", "items": { "type": ["string", "number", "boolean", "null"] } } }
                                },
                                "required": ["rows"]
                            },
                            "image_path": { "type": "string" },
                            "image_width": { "type": "integer", "minimum": 40, "maximum": 1600 },
                            "image_height": { "type": "integer", "minimum": 40, "maximum": 1200 },
                            "page_break": { "type": "boolean" }
                        }
                    }
                }
            },
            "required": ["path", "sections"]
        })
    }
    fn metadata(&self) -> ToolMetadata {
        artifact_metadata()
    }
    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        run_artifact_runtime(ArtifactKind::Document, input, ctx).await
    }
}

pub struct PresentationArtifactTool;

#[async_trait]
impl Tool for PresentationArtifactTool {
    fn name(&self) -> &str {
        "presentation_artifact"
    }
    fn description(&self) -> &str {
        "Create a polished verified PPTX from structured slides with deterministic layouts. Prefer this for new PowerPoint deliverables instead of COM or generated scripts."
    }
    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed(
            "Create a verified PPTX from structured slides; default for new PowerPoint files.",
        )
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "title": { "type": "string" },
                "subtitle": { "type": "string" },
                "author": { "type": "string" },
                "theme": { "type": "string", "enum": ["tech_dark", "clean_light", "business_blue", "editorial"] },
                "slides": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 40,
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "subtitle": { "type": "string" },
                            "body": { "type": "string" },
                            "bullets": { "type": "array", "items": { "type": "string" } },
                            "layout": { "type": "string", "enum": ["section", "bullets", "two_column", "quote", "image", "summary"] },
                            "image_path": { "type": "string" },
                            "accent": { "type": "string" },
                            "left": { "$ref": "#/$defs/column" },
                            "right": { "$ref": "#/$defs/column" }
                        },
                        "required": ["title"]
                    }
                }
            },
            "$defs": {
                "column": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "bullets": { "type": "array", "items": { "type": "string" } }
                    }
                }
            },
            "required": ["path", "title", "slides"]
        })
    }
    fn metadata(&self) -> ToolMetadata {
        artifact_metadata()
    }
    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        run_artifact_runtime(ArtifactKind::Presentation, input, ctx).await
    }
}

pub struct SpreadsheetArtifactTool;

#[async_trait]
impl Tool for SpreadsheetArtifactTool {
    fn name(&self) -> &str {
        "spreadsheet_artifact"
    }
    fn description(&self) -> &str {
        "Create a verified XLSX workbook from structured sheets. Supports columns, rows, formulas, explicit cells, number formats, frozen headers, and filters without Excel COM."
    }
    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed("Create a verified XLSX from structured sheets; default for new Excel files.")
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "creator": { "type": "string" },
                "sheets": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 30,
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "minLength": 1, "maxLength": 31 },
                            "columns": { "type": "array", "items": { "type": "object", "properties": {
                                "header": { "type": "string" }, "key": { "type": "string" }, "width": { "type": "number" },
                                "format": { "type": "string", "enum": ["text", "number", "currency", "percent", "date"] }
                            }, "required": ["header", "key"] } },
                            "rows": { "type": "array", "items": { "type": ["object", "array"] } },
                            "cells": { "type": "array", "items": { "type": "object", "properties": {
                                "cell": { "type": "string" }, "value": {}, "format": { "type": "string", "enum": ["text", "number", "currency", "percent", "date"] }
                            }, "required": ["cell", "value"] } },
                            "freeze_header": { "type": "boolean" },
                            "auto_filter": { "type": "boolean" }
                        },
                        "required": ["name"]
                    }
                }
            },
            "required": ["path", "sheets"]
        })
    }
    fn metadata(&self) -> ToolMetadata {
        artifact_metadata()
    }
    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        run_artifact_runtime(ArtifactKind::Spreadsheet, input, ctx).await
    }
}

fn soffice_candidates() -> Vec<OsString> {
    let mut candidates = Vec::new();
    if let Some(value) = std::env::var_os("WORKBUDDY_SOFFICE").filter(|value| !value.is_empty()) {
        candidates.push(value);
    }
    if cfg!(target_os = "windows") {
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(key) {
                candidates.push(
                    PathBuf::from(root)
                        .join("LibreOffice")
                        .join("program")
                        .join("soffice.exe")
                        .into_os_string(),
                );
            }
        }
        candidates.push(OsString::from("soffice.exe"));
    } else if cfg!(target_os = "macos") {
        candidates.extend([
            OsString::from("/Applications/LibreOffice.app/Contents/MacOS/soffice"),
            OsString::from("/opt/homebrew/bin/soffice"),
            OsString::from("/usr/local/bin/soffice"),
            OsString::from("soffice"),
        ]);
    } else {
        candidates.extend([
            OsString::from("/usr/bin/libreoffice"),
            OsString::from("/usr/bin/soffice"),
            OsString::from("soffice"),
        ]);
    }
    candidates
}

fn find_soffice() -> Result<OsString> {
    soffice_candidates()
        .into_iter()
        .find(|candidate| command_available(candidate, "--version"))
        .ok_or_else(|| {
            anyhow!(
                "LibreOffice was not found. Install LibreOffice or set WORKBUDDY_SOFFICE to enable Office artifact rendering."
            )
        })
}

fn file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = urlencoding::encode(&normalized)
        .replace("%2F", "/")
        .replace("%3A", ":");
    format!("file:///{}", encoded.trim_start_matches('/'))
}

fn find_pdftoppm() -> Option<OsString> {
    let candidates = if cfg!(target_os = "windows") {
        vec![OsString::from("pdftoppm.exe"), OsString::from("pdftoppm")]
    } else {
        vec![
            OsString::from("/opt/homebrew/bin/pdftoppm"),
            OsString::from("/usr/local/bin/pdftoppm"),
            OsString::from("/usr/bin/pdftoppm"),
            OsString::from("pdftoppm"),
        ]
    };
    candidates
        .into_iter()
        .find(|candidate| command_available(candidate, "-v"))
}

pub struct ArtifactRenderTool;

#[async_trait]
impl Tool for ArtifactRenderTool {
    fn name(&self) -> &str {
        "artifact_render"
    }
    fn description(&self) -> &str {
        "Render a DOCX, PPTX, or XLSX to PDF with LibreOffice headless. Optionally rasterize the PDF to PNG pages when pdftoppm is installed. Use this after generation when visual inspection is needed."
    }
    fn description_minimal(&self) -> Cow<'_, str> {
        Cow::Borrowed(
            "Render an Office artifact to PDF or PNG for visual QA; requires LibreOffice.",
        )
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "output_dir": { "type": "string" },
                "format": { "type": "string", "enum": ["pdf", "png"] }
            },
            "required": ["path"]
        })
    }
    fn metadata(&self) -> ToolMetadata {
        artifact_metadata()
    }
    fn needs_confirmation(&self, _input: &Value) -> bool {
        true
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("path is required"))?;
        let source = resolve_path(path, &ctx.workspace_root);
        if !source.is_file() {
            return Ok(ToolResult::err(format!(
                "Artifact does not exist: {}",
                source.display()
            )));
        }
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !["docx", "pptx", "xlsx"]
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return Ok(ToolResult::err(
                "artifact_render supports .docx, .pptx, and .xlsx inputs",
            ));
        }
        let output_dir = input
            .get("output_dir")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| resolve_path(value, &ctx.workspace_root))
            .or_else(|| source.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| ctx.workspace_root.clone());
        std::fs::create_dir_all(&output_dir)?;
        let profile_dir = ctx
            .workspace_root
            .join(".workbuddy")
            .join("artifact-runtime")
            .join(format!("lo-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&profile_dir)?;
        let soffice = match find_soffice() {
            Ok(value) => value,
            Err(error) => return Ok(ToolResult::err(error.to_string())),
        };
        let mut command = Command::new(soffice);
        command
            .arg("--headless")
            .arg("--nologo")
            .arg("--nodefault")
            .arg("--nofirststartwizard")
            .arg(format!("-env:UserInstallation={}", file_url(&profile_dir)))
            .arg("--convert-to")
            .arg("pdf")
            .arg("--outdir")
            .arg(&output_dir)
            .arg(&source)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = match timeout(Duration::from_secs(RENDER_TIMEOUT_SECS), command.output()).await
        {
            Ok(result) => result.context("Failed to start LibreOffice")?,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&profile_dir);
                return Ok(ToolResult::err(format!(
                    "LibreOffice rendering timed out after {RENDER_TIMEOUT_SECS} seconds"
                )));
            }
        };
        let _ = std::fs::remove_dir_all(&profile_dir);
        if !output.status.success() {
            let stderr = trim_process_output(&output.stderr);
            let stdout = trim_process_output(&output.stdout);
            return Ok(ToolResult::err(format!(
                "LibreOffice rendering failed (status={}): {}",
                output.status,
                if stderr.is_empty() { stdout } else { stderr }
            )));
        }
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("artifact");
        let pdf_path = output_dir.join(format!("{stem}.pdf"));
        if !pdf_path.is_file() {
            return Ok(ToolResult::err(format!(
                "LibreOffice completed but did not create {}",
                pdf_path.display()
            )));
        }
        let format = input.get("format").and_then(Value::as_str).unwrap_or("pdf");
        let mut artifacts = vec![ToolArtifact {
            name: pdf_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("artifact.pdf")
                .to_string(),
            artifact_type: "pdf".to_string(),
            uri: Some(pdf_path.to_string_lossy().to_string()),
            content_summary: "Office artifact rendered to PDF".to_string(),
            metadata: None,
        }];
        if format == "png" {
            let Some(pdftoppm) = find_pdftoppm() else {
                return Ok(ToolResult::err(format!(
                    "PDF rendered successfully to {}, but PNG conversion requires pdftoppm (Poppler).",
                    pdf_path.display()
                )));
            };
            let prefix = output_dir.join(format!("{stem}-page"));
            let output = Command::new(pdftoppm)
                .arg("-png")
                .arg("-r")
                .arg("144")
                .arg(&pdf_path)
                .arg(&prefix)
                .output()
                .await?;
            if !output.status.success() {
                return Ok(ToolResult::err(format!(
                    "PNG conversion failed: {}",
                    trim_process_output(&output.stderr)
                )));
            }
            let mut images = std::fs::read_dir(&output_dir)?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .map(|name| {
                            name.starts_with(&format!("{stem}-page")) && name.ends_with(".png")
                        })
                        .unwrap_or(false)
                })
                .collect::<Vec<_>>();
            images.sort();
            artifacts.extend(images.into_iter().map(|path| {
                ToolArtifact {
                    name: path
                        .file_name()
                        .and_then(|value| value.to_str())
                        .unwrap_or("page.png")
                        .to_string(),
                    artifact_type: "image".to_string(),
                    uri: Some(path.to_string_lossy().to_string()),
                    content_summary: "Rendered Office artifact page".to_string(),
                    metadata: None,
                }
            }));
        }
        let payload = json!({
            "ok": true,
            "source": source,
            "pdf": pdf_path,
            "format": format,
            "artifacts": artifacts.len()
        });
        Ok(
            ToolResult::ok(format!("Rendered artifact: {}", pdf_path.display()))
                .with_data(payload)
                .with_artifacts(artifacts),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_kind_enforces_its_ooxml_extension() {
        assert!(validate_artifact_path(ArtifactKind::Document, "report.docx").is_ok());
        assert!(validate_artifact_path(ArtifactKind::Presentation, "deck.pptx").is_ok());
        assert!(validate_artifact_path(ArtifactKind::Spreadsheet, "data.xlsx").is_ok());
        assert!(validate_artifact_path(ArtifactKind::Document, "report.pptx").is_err());
    }

    #[test]
    fn runtime_output_must_report_success_and_the_expected_path() {
        let parsed = parse_runtime_output(
            r#"{"ok":true,"kind":"document","path":"/tmp/report.docx","bytes":1234,"details":{"sections":2}}"#,
            "/tmp/report.docx",
        )
        .expect("valid runtime output");
        assert_eq!(parsed.bytes, 1234);

        assert!(parse_runtime_output(
            r#"{"ok":true,"kind":"document","path":"/tmp/other.docx","bytes":1234,"details":{}}"#,
            "/tmp/report.docx",
        )
        .is_err());
    }

    #[test]
    fn embedded_image_paths_are_resolved_from_the_workspace() {
        let workspace = Path::new("/workspace");
        let mut document = json!({
            "sections": [{ "image_path": "images/report.png" }]
        });
        normalize_embedded_asset_paths(ArtifactKind::Document, &mut document, workspace);
        assert_eq!(
            document["sections"][0]["image_path"],
            Value::String(workspace.join("images/report.png").to_string_lossy().to_string())
        );

        let absolute = workspace.join("cover.png");
        let mut presentation = json!({
            "slides": [{ "image_path": absolute.to_string_lossy() }]
        });
        normalize_embedded_asset_paths(
            ArtifactKind::Presentation,
            &mut presentation,
            workspace,
        );
        assert_eq!(
            presentation["slides"][0]["image_path"],
            Value::String(absolute.to_string_lossy().to_string())
        );
    }

    #[test]
    fn long_documents_can_append_sections_to_a_persisted_draft() {
        let directory = std::env::temp_dir().join(format!(
            "workbuddy-document-append-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory).expect("create tempdir");
        let output = directory.join("report.docx");
        let state = document_state_path(&directory, &output);
        let mut initial = json!({
            "path": output.to_string_lossy(),
            "title": "商业推广报告",
            "sections": [{ "heading": "第一章", "paragraphs": ["初始内容"] }]
        });
        prepare_document_input(&mut initial, &state).expect("initial document input");
        persist_document_state(&state, &initial).expect("persist initial state");

        let mut append = json!({
            "path": output.to_string_lossy(),
            "append": true,
            "sections": [{ "heading": "第二章", "paragraphs": ["追加内容"] }]
        });
        prepare_document_input(&mut append, &state).expect("append document input");

        assert_eq!(append["title"], "商业推广报告");
        assert_eq!(append["sections"].as_array().map(Vec::len), Some(2));
        assert!(append.get("append").is_none());
        let _ = std::fs::remove_dir_all(directory);
    }
}
