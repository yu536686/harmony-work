//! Read-only bounded probes and offline-only repair. No shell or download fallback.
use crate::process::{managed_runtime_executable, tokio_command};
use crate::store::{AppState, Settings};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const REQUIRED_PYTHON_MODULES: &[&str] = &[
    "cv2",
    "numpy",
    "starlette",
    "uvicorn",
    "pydantic",
    "imageio_ffmpeg",
];
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const OUTPUT_LIMIT: usize = 16 * 1024;
const REINSTALL: &str =
    "请使用官方完整离线安装包修复；保留工作区、账号配置和用户数据。轻量包本身不含修复载荷。";

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticItem {
    pub key: String,
    pub name: String,
    pub status: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub details: String,
    pub remediation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticsReport {
    pub schema_version: u32,
    pub checked_at: String,
    pub platform: String,
    pub healthy: bool,
    pub items: Vec<DiagnosticItem>,
}

#[derive(Debug)]
struct Probe {
    status: &'static str,
    stdout: String,
    details: String,
}

// Drain both pipes after the capture limit so noisy children cannot deadlock.
async fn bounded_output(mut reader: impl AsyncRead + Unpin) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let keep = count.min(OUTPUT_LIMIT.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

async fn probe(path: &Path, args: &[&str], input: Option<&str>, deadline: Duration) -> Probe {
    probe_with_environment(path, args, input, deadline, None).await
}

async fn probe_with_environment(
    path: &Path,
    args: &[&str],
    input: Option<&str>,
    deadline: Duration,
    environment: Option<(&str, &std::ffi::OsStr)>,
) -> Probe {
    let mut command = tokio_command(path);
    if let Some((key, value)) = environment {
        command.env(key, value);
    }
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Probe {
                status: "broken",
                stdout: String::new(),
                details: format!("无法启动：{error}"),
            }
        }
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let result = tokio::time::timeout(deadline, async {
        if let Some(input) = input {
            stdin.write_all(input.as_bytes()).await?;
        }
        drop(stdin);
        let (status, stdout, stderr) =
            tokio::join!(child.wait(), bounded_output(stdout), bounded_output(stderr));
        Ok::<_, std::io::Error>((status?, stdout?, stderr?))
    })
    .await;
    match result {
        Ok(Ok((status, stdout, stderr))) => Probe {
            status: if status.success() { "ok" } else { "broken" },
            details: if status.success() {
                "进程检查通过".into()
            } else {
                format!("退出码 {:?}：{}", status.code(), stderr)
            },
            stdout,
        },
        Ok(Err(error)) => Probe {
            status: "broken",
            stdout: String::new(),
            details: format!("进程读取失败：{error}"),
        },
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_millis(300), child.wait()).await;
            Probe {
                status: "timeout",
                stdout: String::new(),
                details: format!("检查超过 {} 秒，已终止子进程", deadline.as_secs_f32()),
            }
        }
    }
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    let path = crate::process::managed_path_value().or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) {
            for extension in ["exe", "cmd", "bat"] {
                let candidate = dir.join(format!("{command}.{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

fn resolve_runtime(key: &str, paths: &HashMap<String, String>) -> Option<PathBuf> {
    // Do not hide a broken explicit override behind a different interpreter.
    if let Some(path) = paths.get(key).filter(|path| !path.trim().is_empty()) {
        return Some(PathBuf::from(path));
    }
    managed_runtime_executable(key)
        .or_else(|| find_on_path(if key == "wecom_cli" { "wecom-cli" } else { key }))
        .or_else(|| (key == "python").then(|| find_on_path("python3")).flatten())
}

fn missing(key: &str, name: &str) -> DiagnosticItem {
    DiagnosticItem {
        key: key.into(),
        name: name.into(),
        status: "missing".into(),
        path: None,
        version: None,
        details: "未找到可用路径".into(),
        remediation: REINSTALL.into(),
    }
}

fn version_output(value: &str) -> bool {
    regex::Regex::new(r"\d+\.\d+(?:\.\d+)?")
        .expect("version regex")
        .is_match(value)
}

async fn version_item(key: &str, name: &str, path: Option<PathBuf>) -> DiagnosticItem {
    let Some(path) = path else {
        return missing(key, name);
    };
    let mut item = missing(key, name);
    item.path = Some(path.to_string_lossy().into_owned());
    if !path.is_file() {
        return item;
    }
    let result = probe(&path, &["--version"], None, PROBE_TIMEOUT).await;
    item.status = result.status.into();
    item.details = result.details;
    if item.status == "ok" {
        if version_output(&result.stdout) {
            item.version = Some(result.stdout);
        } else {
            item.status = "broken".into();
            item.details = "版本命令未返回有效版本；不能仅凭文件存在判定可用".into();
        }
    }
    item
}

async fn python_items(path: Option<PathBuf>) -> Vec<DiagnosticItem> {
    let mut items = vec![version_item("python", "Python", path.clone()).await];
    let Some(path) = path.filter(|path| path.is_file()) else {
        items.extend(
            REQUIRED_PYTHON_MODULES
                .iter()
                .map(|module| missing(&format!("python:{module}"), &format!("Python / {module}"))),
        );
        return items;
    };
    let script = format!("import importlib,json\nr=[]\nfor name in {}:\n try:\n  m=importlib.import_module(name); r.append({{'name':name,'ok':True,'version':str(getattr(m,'__version__','unknown'))}})\n except Exception as e:\n  r.append({{'name':name,'ok':False,'error':type(e).__name__+': '+str(e)}})\nprint(json.dumps(r))", serde_json::to_string(REQUIRED_PYTHON_MODULES).expect("module names"));
    let result = probe(&path, &["-I", "-c", &script], None, Duration::from_secs(10)).await;
    let modules =
        serde_json::from_str::<Vec<serde_json::Value>>(&result.stdout).unwrap_or_default();
    for module in REQUIRED_PYTHON_MODULES {
        let mut item = missing(&format!("python:{module}"), &format!("Python / {module}"));
        item.path = Some(path.to_string_lossy().into_owned());
        let found = modules
            .iter()
            .find(|value| value["name"].as_str() == Some(module));
        item.status = if result.status != "ok" {
            result.status
        } else if found.is_some_and(|value| value["ok"] == true) {
            "ok"
        } else {
            "broken"
        }
        .into();
        item.version = found
            .and_then(|value| value["version"].as_str())
            .map(str::to_owned);
        item.details = if item.status == "ok" {
            "实际导入成功".into()
        } else {
            found
                .and_then(|value| value["error"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("无法验证模块导入：{}", result.details))
        };
        items.push(item);
    }
    items
}

fn resource_candidates(relative: &str) -> Vec<PathBuf> {
    let exe = std::env::current_exe().unwrap_or_default();
    let directory = exe.parent().unwrap_or_else(|| Path::new("."));
    let mut roots = vec![directory.join("resources"), directory.to_path_buf()];
    if let Some(contents) = directory
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "Contents"))
    {
        roots.push(contents.join("Resources/resources"));
        roots.push(contents.join("Resources"));
    }
    if !exe.to_string_lossy().contains(".app/") && cfg!(debug_assertions) {
        roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources"));
    }
    roots.into_iter().map(|root| root.join(relative)).collect()
}

fn resource(relative: &str) -> Option<PathBuf> {
    resource_candidates(relative)
        .into_iter()
        .find(|path| path.is_file())
}

async fn wecom_item(node: Option<PathBuf>, paths: &HashMap<String, String>) -> DiagnosticItem {
    let Some(path) = resolve_runtime("wecom_cli", paths) else {
        return missing("wecom_cli", "WeCom CLI");
    };
    if cfg!(windows)
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd"))
    {
        let entry = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("node_modules/@wecom/cli/bin/wecom.js");
        let mut item = missing("wecom_cli", "WeCom CLI");
        item.path = Some(path.to_string_lossy().into_owned());
        if let Some(node) = node.filter(|_| entry.is_file()) {
            let result = probe(
                &node,
                &[entry.to_string_lossy().as_ref(), "--version"],
                None,
                PROBE_TIMEOUT,
            )
            .await;
            item.status = result.status.into();
            item.details = result.details;
            if result.status == "ok" && version_output(&result.stdout) {
                item.version = Some(result.stdout);
            } else if result.status == "ok" {
                item.status = "broken".into();
                item.details = "WeCom 未返回有效版本".into();
            }
        }
        return item;
    }
    version_item("wecom_cli", "WeCom CLI", Some(path)).await
}

async fn mcp_host_item() -> DiagnosticItem {
    let binary = if cfg!(windows) {
        "workbuddy-mcp-host.exe"
    } else {
        "workbuddy-mcp-host"
    };
    let path = resource(binary).or_else(|| resource(&format!("workbuddy-mcp-host/{binary}")));
    let Some(path) = path else {
        return missing("mcp_host", "MCP Host");
    };
    let mut item = missing("mcp_host", "MCP Host");
    item.path = Some(path.to_string_lossy().into_owned());
    let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"diagnostics\",\"version\":\"1\"}}}\n";
    let result = probe(&path, &[], Some(input), PROBE_TIMEOUT).await;
    let response = result
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["id"] == 1);
    let version = response
        .as_ref()
        .and_then(|value| value["result"]["serverInfo"]["version"].as_str());
    item.status = if result.status != "ok" {
        result.status
    } else if version.is_some_and(version_output) {
        "ok"
    } else {
        "broken"
    }
    .into();
    item.version = version.map(str::to_owned);
    item.details = if item.status == "ok" {
        "MCP initialize 握手成功".into()
    } else {
        format!("MCP 握手未通过：{}", result.details)
    };
    item
}

async fn playwright_item(node: Option<PathBuf>) -> DiagnosticItem {
    let Some(entry) = resource("playwright-mcp/cli.js") else {
        return missing("playwright_mcp", "Browser MCP");
    };
    let mut item = missing("playwright_mcp", "Browser MCP");
    item.path = Some(entry.to_string_lossy().into_owned());
    if let Some(node) = node {
        let result = probe(
            &node,
            &[entry.to_string_lossy().as_ref(), "--version"],
            None,
            PROBE_TIMEOUT,
        )
        .await;
        item.status = result.status.into();
        item.details = result.details;
        if result.status == "ok" && version_output(&result.stdout) {
            item.version = Some(result.stdout);
        } else if result.status == "ok" {
            item.status = "broken".into();
            item.details = "Browser MCP 未返回有效版本".into();
        }
    } else {
        item.details = "Browser MCP 入口存在，但缺少 Node，无法验证模块和版本".into();
    }
    item
}

async fn browser_item() -> DiagnosticItem {
    let Some(browser) = crate::browser_runtime::installed_browser() else {
        return missing("browser", "Browser (Chrome / Edge)");
    };
    let mut item = missing("browser", browser.name);
    item.path = Some(browser.executable.to_string_lossy().into_owned());
    // Chrome/Edge --version can open a browser window on Windows; read its actual file metadata instead.
    #[cfg(windows)]
    let result = if let Some(powershell) = find_on_path("powershell.exe") {
        probe_with_environment(&powershell, &["-NoProfile", "-NonInteractive", "-Command", "[System.Diagnostics.FileVersionInfo]::GetVersionInfo($env:HARMONY_DIAGNOSTICS_BROWSER).ProductVersion"], None, PROBE_TIMEOUT, Some(("HARMONY_DIAGNOSTICS_BROWSER", browser.executable.as_os_str()))).await
    } else {
        Probe {
            status: "broken",
            stdout: String::new(),
            details: "无法读取浏览器文件版本：PowerShell 不可用".into(),
        }
    };
    #[cfg(target_os = "macos")]
    let result = {
        let plist = browser
            .executable
            .parent()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new(""))
            .join("Info.plist");
        probe(
            Path::new("/usr/libexec/PlistBuddy"),
            &[
                "-c",
                "Print :CFBundleShortVersionString",
                plist.to_string_lossy().as_ref(),
            ],
            None,
            PROBE_TIMEOUT,
        )
        .await
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let result = probe(&browser.executable, &["--version"], None, PROBE_TIMEOUT).await;
    item.status = result.status.into();
    item.details = result.details;
    if result.status == "ok" && version_output(&result.stdout) {
        item.version = Some(result.stdout);
    } else if result.status == "ok" {
        item.status = "broken".into();
        item.details = "浏览器存在，但版本读取失败".into();
    }
    item.remediation = "请安装或修复官方 Chrome / Edge；不会自动下载或更改浏览器数据。".into();
    item
}

pub async fn collect(settings: &Settings) -> DiagnosticsReport {
    let node = resolve_runtime("node", &settings.runtime_paths);
    let python = resolve_runtime("python", &settings.runtime_paths);
    let (node_item, python, wecom, browser, host, playwright) = tokio::join!(
        version_item("node", "Node.js", node.clone()),
        python_items(python),
        wecom_item(node.clone(), &settings.runtime_paths),
        browser_item(),
        mcp_host_item(),
        playwright_item(node)
    );
    let mut items = vec![node_item];
    items.extend(python);
    items.extend([wecom, browser, host, playwright]);
    DiagnosticsReport {
        schema_version: 1,
        checked_at: chrono::Utc::now().to_rfc3339(),
        platform: std::env::consts::OS.into(),
        healthy: items.iter().all(|item| item.status == "ok"),
        items,
    }
}

fn redact_text(value: &str) -> String {
    let mut value = value.to_owned();
    if let Some(home) = dirs::home_dir().and_then(|path| path.to_str().map(str::to_owned)) {
        value = value
            .replace(&home, "[USER_HOME]")
            .replace(&home.replace('\\', "/"), "[USER_HOME]");
    }
    for pattern in [
        r#"(?i)(?:[a-z]:)?[/\\](?:users|home)[/\\][^/\\"'<>]+"#,
        r#"(?i)(?:api[_-]?key|access[_-]?token|refresh[_-]?token|key|token|password|passwd|secret|authorization)["']?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|(?:Bearer\s+)?[^\s,;"}]+)"#,
        r#"(?i)\bBearer\s+[^\s,;"}]+"#,
        r"\bsk-[a-zA-Z0-9_-]{8,}\b",
        r"(?i)://[^/@\s]+:[^/@\s]+@",
    ] {
        if let Ok(regex) = regex::Regex::new(pattern) {
            value = regex.replace_all(&value, "[REDACTED]").into_owned();
        }
    }
    value
}

fn redacted_report(report: &DiagnosticsReport) -> String {
    // Export only this typed allowlist, never Settings, MCP env/headers, or raw logs.
    let mut safe = report.clone();
    for item in &mut safe.items {
        item.path = item.path.as_deref().map(redact_text);
        item.version = item.version.as_deref().and_then(|value| {
            regex::Regex::new(r"\d+\.\d+(?:\.\d+)?(?:-[a-zA-Z0-9.]+)?")
                .expect("version regex")
                .find(value)
                .map(|version| version.as_str().to_owned())
        });
        // Exception text may contain unlabeled secrets. Keep it on the local screen only.
        item.details = match item.status.as_str() {
            "ok" => "检查通过",
            "missing" => "路径或依赖缺失",
            "timeout" => "进程检查超时",
            _ => "检查失败；请在本机查看详情",
        }
        .into();
        item.remediation = redact_text(&item.remediation);
    }
    serde_json::to_string_pretty(&safe).expect("serializable diagnostics")
}

#[tauri::command]
pub async fn check_runtime_diagnostics(
    state: State<'_, AppState>,
) -> Result<DiagnosticsReport, String> {
    let settings = state.settings.lock().await.clone();
    Ok(collect(&settings).await)
}

#[tauri::command]
pub async fn export_runtime_diagnostics(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let settings = state.settings.lock().await.clone();
    let content = redacted_report(&collect(&settings).await);
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("保存脱敏诊断报告")
            .set_file_name("harmony-runtime-diagnostics.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|_| "保存位置无效".to_string())?;
        std::fs::write(path, content)
            .map_err(|_| "诊断报告保存失败，请检查文件权限".to_string())?;
        Ok(true)
    })
    .await
    .map_err(|_| "保存任务失败".to_string())?
}

pub async fn repair_offline_environment(
    settings: &Settings,
) -> Result<super::EnvironmentBootstrapResult, String> {
    let before = collect(settings).await;
    if before
        .items
        .iter()
        .any(|item| item.key.starts_with("python") && item.status != "ok")
    {
        #[cfg(windows)]
        {
            if settings
                .runtime_paths
                .get("python")
                .is_some_and(|path| !path.trim().is_empty())
            {
                return Err("Python 使用自定义路径，请先在原有运行环境设置中切回内置 Python，再运行离线修复。".into());
            }
            tauri::async_runtime::spawn_blocking(repair_cached_python)
                .await
                .map_err(|_| "离线修复任务失败".to_string())??;
        }
        #[cfg(not(windows))]
        return Err(REINSTALL.into());
    }
    let after = collect(settings).await;
    let failed = after
        .items
        .iter()
        .filter(|item| item.status != "ok")
        .map(|item| item.name.as_str())
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "离线复检仍需处理：{}。{REINSTALL}",
            failed.join("、")
        ));
    }
    Ok(super::EnvironmentBootstrapResult {
        items: after
            .items
            .into_iter()
            .map(|item| super::EnvironmentBootstrapItem {
                key: item.key,
                name: item.name,
                available: item.status == "ok",
                version: item.version,
                local_installer: None,
                source: "local".into(),
                status: "ready".into(),
                details: item.details,
            })
            .collect(),
        logs: vec!["严格离线复检通过；未联网、未删除用户数据。".into()],
        workspace_root: settings.workspace_root.clone(),
        offline_dirs: vec![],
    })
}

#[cfg(windows)]
fn repair_cached_python() -> Result<(), String> {
    let root = crate::process::managed_runtime_root();
    let cache = root
        .parent()
        .ok_or("无法定位安装器缓存")?
        .join("installer-cache");
    let version = env!("CARGO_PKG_VERSION");
    merge_cached_python_from_cache(&cache, &root.join("python"), version)
}

// Test on macOS with isolated fixtures, never the user's installed runtimes.
#[cfg(any(windows, test))]
const FROZEN_PYTHON_SHA256: &str =
    "83643dc01595eba166e84620d8a2e999796d6cfb1abb051101febedc627ced96";
#[cfg(any(windows, test))]
const COMPATIBLE_PYTHON_CACHE_VERSIONS: &[&str] = &["1.1.2", "1.1.1", "1.1.0"];

#[cfg(any(windows, test))]
fn permits_frozen_python_cache_reuse(version: &str) -> bool {
    matches!(version, "1.1.3-rc1" | "1.1.3-rc2" | "1.1.3")
}

#[cfg(any(windows, test))]
fn merge_cached_python_from_cache(
    cache: &Path,
    destination: &Path,
    version: &str,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut error = format!("没有可验证的完整离线载荷。{REINSTALL}");
    // Only these light candidates may reuse the explicitly frozen full payloads.
    let previous = COMPATIBLE_PYTHON_CACHE_VERSIONS
        .iter()
        .copied()
        .filter(|_| permits_frozen_python_cache_reuse(version));
    for cached_version in std::iter::once(version).chain(previous) {
        let payload = cache.join(cached_version).join("payload");
        match verified_python_archive(&payload, version, Some(cached_version), started) {
            Ok(file) => return merge_python_archive(file, destination, started),
            Err(reason) => error = reason,
        }
    }
    Err(error)
}

#[cfg(any(windows, test))]
fn read_payload_manifest(payload: &Path) -> Result<serde_json::Value, String> {
    use std::io::Read;
    const LIMIT: u64 = 1024 * 1024;
    let file = std::fs::File::open(payload.join("payload.manifest.json"))
        .map_err(|_| format!("没有本版本完整离线载荷。{REINSTALL}"))?;
    let metadata = file.metadata().map_err(|_| "无法读取离线载荷清单")?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err("离线载荷清单大小超限或不是普通文件".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取离线载荷清单")?;
    if bytes.len() as u64 > LIMIT {
        return Err("离线载荷清单大小超限".into());
    }
    serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes))
        .map_err(|_| "离线载荷清单无效".into())
}

#[cfg(test)]
fn merge_cached_python(payload: &Path, destination: &Path, version: &str) -> Result<(), String> {
    let started = std::time::Instant::now();
    let file = verified_python_archive(payload, version, None, started)?;
    merge_python_archive(file, destination, started)
}

#[cfg(any(windows, test))]
fn verified_python_archive(
    payload: &Path,
    version: &str,
    cached_version: Option<&str>,
    started: std::time::Instant,
) -> Result<std::fs::File, String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Seek, SeekFrom};
    let manifest = read_payload_manifest(payload)?;
    let payload_version = manifest["version"].as_str().ok_or("离线载荷版本缺失")?;
    if cached_version.is_some_and(|expected| expected != payload_version) {
        return Err("离线载荷版本不匹配，拒绝修复".into());
    }
    let previous = payload_version != version;
    if previous
        && (!permits_frozen_python_cache_reuse(version)
            || !COMPATIBLE_PYTHON_CACHE_VERSIONS.contains(&payload_version))
    {
        return Err("离线载荷版本不匹配，拒绝修复".into());
    }
    let entry = manifest["files"]
        .as_array()
        .and_then(|files| {
            files.iter().find(|file| {
                file["path"]
                    .as_str()
                    .is_some_and(|path| path.replace('\\', "/") == "runtime/python-runtime.zip")
            })
        })
        .ok_or("离线载荷缺少 Python 记录")?;
    let expected = entry["sha256"].as_str().ok_or("Python 载荷缺少校验和")?;
    if previous && !expected.eq_ignore_ascii_case(FROZEN_PYTHON_SHA256) {
        return Err("旧版本 Python 载荷不匹配冻结校验和，拒绝修复".into());
    }
    let mut file = std::fs::File::open(payload.join("runtime/python-runtime.zip"))
        .map_err(|_| "Python 离线载荷不存在")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        if started.elapsed() > Duration::from_secs(60) {
            return Err("离线校验超时".into());
        }
        let count = file.read(&mut buffer).map_err(|_| "读取 Python 载荷失败")?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected)
        || (previous && !actual.eq_ignore_ascii_case(FROZEN_PYTHON_SHA256))
    {
        return Err("Python 载荷 SHA-256 校验失败，未修改运行环境".into());
    }
    file.seek(SeekFrom::Start(0)).map_err(|_| "载荷定位失败")?;
    Ok(file)
}

#[cfg(any(windows, test))]
fn python_archive_relative_path(name: &str) -> Result<PathBuf, String> {
    // The frozen Windows ZIP uses backslashes. Validate after normalizing them.
    let normalized = name.replace('\\', "/");
    let normalized = normalized.strip_suffix('/').unwrap_or(&normalized);
    let relative = PathBuf::from(normalized);
    if normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part.ends_with('.') || part.ends_with(' '))
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("ZIP 含越界或非法路径".into());
    }
    Ok(relative)
}

#[cfg(any(windows, test))]
fn merge_python_archive(
    file: std::fs::File,
    destination: &Path,
    started: std::time::Instant,
) -> Result<(), String> {
    use std::io::Read;
    let mut buffer = [0_u8; 65536];
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "Python 载荷不是有效 ZIP")?;
    if zip.len() > 30000 {
        return Err("离线包文件数超限".into());
    }
    let mut total = 0_u64;
    // Validate all members before writing. Do not follow existing symlinks/junctions.
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|_| "ZIP 目录损坏")?;
        entry.enclosed_name().ok_or("ZIP 含越界路径")?;
        let relative = python_archive_relative_path(entry.name())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("ZIP 含非法路径或符号链接".into());
        }
        total = total.saturating_add(entry.size());
        if total > 2 * 1024 * 1024 * 1024 {
            return Err("离线包展开大小超限".into());
        }
        let target = destination.join(relative);
        for ancestor in target.ancestors() {
            if std::fs::symlink_metadata(ancestor).is_ok_and(|meta| is_link(&meta)) {
                return Err("修复目标含符号链接或目录联接".into());
            }
        }
    }
    for index in 0..zip.len() {
        if started.elapsed() > Duration::from_secs(60) {
            return Err("离线修复超时，请使用完整安装包".into());
        }
        let mut entry = zip.by_index(index).map_err(|_| "读取 ZIP 失败")?;
        let target = destination.join(python_archive_relative_path(entry.name())?);
        if entry.is_dir() {
            std::fs::create_dir_all(target).map_err(|_| "无法创建运行环境目录")?;
            continue;
        }
        std::fs::create_dir_all(target.parent().ok_or("目标路径无效")?)
            .map_err(|_| "创建目标目录失败")?;
        let temporary = target.with_extension(format!("repair-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| "无法写入修复文件")?;
            loop {
                if started.elapsed() > Duration::from_secs(60) {
                    return Err("离线修复超时".into());
                }
                let count = entry.read(&mut buffer).map_err(|_| "解压修复文件失败")?;
                if count == 0 {
                    break;
                }
                use std::io::Write;
                output
                    .write_all(&buffer[..count])
                    .map_err(|_| "写入修复文件失败")?;
            }
            drop(output);
            std::fs::copy(&temporary, &target)
                .map_err(|_| "运行环境被占用或不可写，请关闭相关任务后重试")?;
            Ok::<(), String>(())
        })();
        let _ = std::fs::remove_file(temporary);
        result?;
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_health_requires_real_imports_for_every_python_dependency() {
        assert_eq!(
            REQUIRED_PYTHON_MODULES,
            &[
                "cv2",
                "numpy",
                "starlette",
                "uvicorn",
                "pydantic",
                "imageio_ffmpeg"
            ]
        );
    }

    #[test]
    fn runtime_health_export_redacts_secrets_and_username_paths() {
        let result = redact_text("api_key=abc token=def password=xyz /Users/alice/work C:\\Users\\bob\\app /home/carol/work Authorization: Bearer sensitive");
        for secret in ["abc", "def", "xyz", "alice", "bob", "carol", "sensitive"] {
            assert!(!result.contains(secret), "leaked {secret}: {result}");
        }
        let quoted = redact_text(r#"{"Key":"key with spaces","Token":"token secret","password":"pass phrase"} C:\Users\Alice Smith\app /Users/Alice Smith/app"#);
        for secret in ["key with spaces", "token secret", "pass phrase", "Alice", "Smith"] {
            assert!(!quoted.contains(secret), "leaked {secret}: {quoted}");
        }
    }

    #[test]
    fn runtime_health_does_not_hide_broken_custom_paths() {
        let paths = HashMap::from([("python".into(), "/nonexistent/custom-python".into())]);
        assert_eq!(
            resolve_runtime("python", &paths),
            Some(PathBuf::from("/nonexistent/custom-python"))
        );
    }

    #[tokio::test]
    async fn runtime_health_missing_file_is_not_healthy() {
        let item = version_item("node", "Node", Some(PathBuf::from("/nonexistent/node"))).await;
        assert_eq!(item.status, "missing");
        assert!(item.version.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runtime_health_kills_hanging_probe_and_bounds_output() {
        let start = std::time::Instant::now();
        let result = probe(
            Path::new("/bin/sh"),
            &["-c", "while :; do printf abcdefghijklmnopqrstuvwxyz; done"],
            None,
            Duration::from_millis(100),
        )
        .await;
        assert_eq!(result.status, "timeout");
        assert!(start.elapsed() < Duration::from_secs(2));
        let result = probe(Path::new("/bin/echo"), &["1.2.3"], None, PROBE_TIMEOUT).await;
        assert_eq!(result.status, "ok");
        assert_eq!(result.stdout, "1.2.3");
    }

    #[test]
    fn runtime_health_export_never_serializes_raw_exception_output() {
        let mut item = missing("python:numpy", "Python / numpy");
        item.details = "unlabeled-credential-value".into();
        item.path = Some("C:\\Users\\alice\\python.exe".into());
        let report = DiagnosticsReport {
            schema_version: 1,
            checked_at: "now".into(),
            platform: "windows".into(),
            healthy: false,
            items: vec![item],
        };
        let text = redacted_report(&report);
        assert!(!text.contains("unlabeled-credential-value"));
        assert!(!text.contains("alice"));
        assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
    }

    fn repair_fixture(entries: &[(&str, &str)]) -> (PathBuf, PathBuf, PathBuf) {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("diagnostics-test-{}", uuid::Uuid::new_v4()));
        let payload = root.join("payload");
        let destination = root.join("python");
        std::fs::create_dir_all(payload.join("runtime")).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let root = root.canonicalize().unwrap();
        let payload = root.join("payload");
        let destination = root.join("python");
        let archive = payload.join("runtime/python-runtime.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for (name, content) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let hash = hex::encode(Sha256::digest(std::fs::read(archive).unwrap()));
        std::fs::write(payload.join("payload.manifest.json"), serde_json::json!({"version":"test", "files":[{"path":"runtime/python-runtime.zip","sha256":hash}]}).to_string()).unwrap();
        (root, payload, destination)
    }

    fn set_repair_manifest(payload: &Path, version: &str, hash: Option<&str>) {
        let path = payload.join("payload.manifest.json");
        let mut manifest = read_payload_manifest(payload).unwrap();
        manifest["version"] = version.into();
        if let Some(hash) = hash {
            manifest["files"][0]["sha256"] = hash.into();
        }
        std::fs::write(path, manifest.to_string()).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_selects_current_cache_first() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "current")]);
        let current = root.join("installer-cache/test/payload");
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::rename(payload, current).unwrap();
        merge_cached_python_from_cache(&root.join("installer-cache"), &destination, "test")
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("python.exe")).unwrap(),
            "current"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rejects_self_claimed_previous_hashes() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "untrusted")]);
        let own_hash = read_payload_manifest(&payload).unwrap()["files"][0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        std::fs::write(destination.join("user-data.txt"), "keep").unwrap();
        for version in ["1.1.0", "1.1.1", "1.1.2", "0.9.0", "1.1.4"] {
            set_repair_manifest(&payload, version, Some(&own_hash));
            assert!(merge_cached_python(&payload, &destination, "1.1.3-rc1").is_err());
            set_repair_manifest(&payload, version, Some(FROZEN_PYTHON_SHA256));
            assert!(merge_cached_python(&payload, &destination, "1.1.3-rc1").is_err());
        }
        assert!(!destination.join("python.exe").exists());
        assert_eq!(
            std::fs::read_to_string(destination.join("user-data.txt")).unwrap(),
            "keep"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rejects_mislabeled_cache_directory() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "untrusted")]);
        set_repair_manifest(&payload, "1.1.3-rc1", None);
        let previous = root.join("installer-cache/1.1.0/payload");
        std::fs::create_dir_all(previous.parent().unwrap()).unwrap();
        std::fs::rename(payload, previous).unwrap();
        assert!(merge_cached_python_from_cache(
            &root.join("installer-cache"),
            &destination,
            "1.1.3-rc1"
        )
        .is_err());
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rejects_archive_symlink_before_writing() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "new")]);
        let archive = payload.join("runtime/python-runtime.zip");
        let mut writer = zip::ZipWriter::new_append(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&archive)
                .unwrap(),
        )
        .unwrap();
        writer
            .add_symlink("Lib", "outside", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        assert!(merge_python_archive(
            std::fs::File::open(archive).unwrap(),
            &destination,
            std::time::Instant::now()
        )
        .unwrap_err()
        .contains("符号链接"));
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_preserves_archive_count_limit() {
        let (root, payload, destination) = repair_fixture(&[]);
        let archive = payload.join("runtime/python-runtime.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for index in 0..30001 {
            writer
                .start_file(
                    format!("file-{index}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
        }
        writer.finish().unwrap();
        assert!(merge_python_archive(
            std::fs::File::open(archive).unwrap(),
            &destination,
            std::time::Instant::now()
        )
        .unwrap_err()
        .contains("文件数超限"));
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_preserves_expanded_size_limit() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "new")]);
        let mut bytes = std::fs::read(payload.join("runtime/python-runtime.zip")).unwrap();
        let directory = bytes
            .windows(4)
            .position(|part| part == b"PK\x01\x02")
            .unwrap();
        bytes[directory + 24..directory + 28]
            .copy_from_slice(&(2_u32 * 1024 * 1024 * 1024 + 1).to_le_bytes());
        let archive = root.join("oversized.zip");
        std::fs::write(&archive, bytes).unwrap();
        assert!(merge_python_archive(
            std::fs::File::open(archive).unwrap(),
            &destination,
            std::time::Instant::now()
        )
        .unwrap_err()
        .contains("展开大小超限"));
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_preserves_hash_timeout() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "new")]);
        assert!(verified_python_archive(
            &payload,
            "test",
            None,
            std::time::Instant::now() - Duration::from_secs(61)
        )
        .unwrap_err()
        .contains("超时"));
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rejects_windows_and_mixed_traversal() {
        for name in [
            "..\\escape",
            "Lib\\..\\escape",
            "Lib/../../escape",
            "\\\\server\\share",
            "\\absolute",
            "/absolute",
            "C:\\escape",
            "file:stream",
            "./escape",
            "Lib\\.. \\escape",
            "Lib\\...\\escape",
        ] {
            let (root, payload, destination) =
                repair_fixture(&[("python.exe", "new"), (name, "bad")]);
            assert!(
                merge_cached_python(&payload, &destination, "test").is_err(),
                "accepted {name}"
            );
            assert!(!destination.join("python.exe").exists());
            assert!(!root.join("escape").exists());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn runtime_health_offline_repair_accepts_utf8_bom_manifest() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "fixed")]);
        let manifest = payload.join("payload.manifest.json");
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(std::fs::read(&manifest).unwrap());
        std::fs::write(manifest, bytes).unwrap();
        let result = merge_cached_python(&payload, &destination, "test");
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok(), "BOM manifest rejected: {result:?}");
    }

    #[test]
    fn runtime_health_offline_repair_bounds_manifest_before_writing() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "fixed")]);
        let manifest = payload.join("payload.manifest.json");
        let mut bytes = std::fs::read(&manifest).unwrap();
        bytes.resize(1024 * 1024 + 1, b' ');
        std::fs::write(manifest, bytes).unwrap();
        let result = merge_cached_python(&payload, &destination, "test");
        let wrote_python = destination.join("python.exe").exists();
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_err(), "oversized manifest accepted");
        assert!(!wrote_python);
    }

    #[test]
    fn runtime_health_offline_repair_accepts_safe_windows_zip_separators() {
        let (root, payload, destination) =
            repair_fixture(&[("Lib\\site-packages\\numpy\\__init__.py", "fixed")]);
        let result = merge_cached_python(&payload, &destination, "test");
        let repaired = destination
            .join("Lib/site-packages/numpy/__init__.py")
            .is_file();
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok(), "Windows ZIP rejected: {result:?}");
        assert!(repaired);
    }

    #[test]
    fn runtime_health_offline_repair_rc2_requires_frozen_previous_hashes() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "untrusted")]);
        let own_hash = read_payload_manifest(&payload).unwrap()["files"][0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        for cached_version in COMPATIBLE_PYTHON_CACHE_VERSIONS {
            set_repair_manifest(&payload, cached_version, Some(&own_hash));
            let error = verified_python_archive(
                &payload,
                "1.1.3-rc2",
                Some(cached_version),
                std::time::Instant::now(),
            )
            .unwrap_err();
            assert!(
                error.contains("冻结校验和"),
                "unexpected rejection: {error}"
            );
            set_repair_manifest(&payload, cached_version, Some(FROZEN_PYTHON_SHA256));
            let error = verified_python_archive(
                &payload,
                "1.1.3-rc2",
                Some(cached_version),
                std::time::Instant::now(),
            )
            .unwrap_err();
            assert!(error.contains("SHA-256"), "unexpected rejection: {error}");
        }
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_formal_113_reuses_only_explicitly_allowed_frozen_cache() {
        assert!(permits_frozen_python_cache_reuse("1.1.3"));
        assert!(!permits_frozen_python_cache_reuse("1.1.4"));
        let (root, payload, _) = repair_fixture(&[("python.exe", "untrusted")]);
        set_repair_manifest(&payload, "1.1.2", Some(FROZEN_PYTHON_SHA256));
        let error = verified_python_archive(&payload, "1.1.3", Some("1.1.2"), std::time::Instant::now()).unwrap_err();
        assert!(error.contains("SHA-256"), "unexpected rejection: {error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rc2_rejects_unsupported_cache_and_target_versions() {
        let (root, payload, destination) = repair_fixture(&[("python.exe", "untrusted")]);
        for cached_version in ["0.9.0", "1.1.3-rc1", "1.1.3-rc3", "1.1.4"] {
            set_repair_manifest(&payload, cached_version, Some(FROZEN_PYTHON_SHA256));
            let error = verified_python_archive(
                &payload,
                "1.1.3-rc2",
                Some(cached_version),
                std::time::Instant::now(),
            )
            .unwrap_err();
            assert!(
                error.contains("版本不匹配"),
                "accepted unsupported cache: {cached_version}"
            );
        }
        set_repair_manifest(&payload, "1.1.2", Some(FROZEN_PYTHON_SHA256));
        for target in ["1.1.3-rc0", "1.1.3-rc02", "1.1.3-rc3", "1.1.4-rc1"] {
            let error =
                verified_python_archive(&payload, target, Some("1.1.2"), std::time::Instant::now())
                    .unwrap_err();
            assert!(
                error.contains("版本不匹配"),
                "accepted unsupported target: {target}"
            );
        }
        assert!(verified_python_archive(
            &payload,
            "1.1.3-rc2",
            Some("1.1.0"),
            std::time::Instant::now()
        )
        .unwrap_err()
        .contains("版本不匹配"));
        assert!(!destination.join("python.exe").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires independently hashed production payload; set HARMONY_FROZEN_PYTHON_TEST_PAYLOAD"]
    fn runtime_health_offline_repair_rc2_accepts_verified_frozen_previous_payload() {
        let fixture = PathBuf::from(
            std::env::var_os("HARMONY_FROZEN_PYTHON_TEST_PAYLOAD")
                .expect("production fixture payload"),
        );
        let (root, _, destination) = repair_fixture(&[]);
        let payload = root.join("installer-cache/1.1.2/payload");
        std::fs::create_dir_all(payload.join("runtime")).unwrap();
        for relative in ["payload.manifest.json", "runtime/python-runtime.zip"] {
            std::fs::copy(fixture.join(relative), payload.join(relative)).unwrap();
        }
        set_repair_manifest(&payload, "1.1.2", None);
        std::fs::write(destination.join("user-data.txt"), "keep").unwrap();
        assert!(!root.join("installer-cache/1.1.3-rc2").exists());
        let result = merge_cached_python_from_cache(
            &root.join("installer-cache"),
            &destination,
            "1.1.3-rc2",
        );
        let repaired = destination.join("python.exe").is_file();
        let modules_repaired = REQUIRED_PYTHON_MODULES.iter().all(|module| {
            destination
                .join(format!("Lib/site-packages/{module}/__init__.py"))
                .is_file()
        });
        let preserved = std::fs::read_to_string(destination.join("user-data.txt")).unwrap();
        for cached_version in COMPATIBLE_PYTHON_CACHE_VERSIONS {
            set_repair_manifest(&payload, cached_version, None);
            let verified = verified_python_archive(
                &payload,
                "1.1.3-rc2",
                Some(cached_version),
                std::time::Instant::now(),
            );
            assert!(
                verified.is_ok(),
                "rc2 rejected frozen {cached_version}: {verified:?}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
        assert!(result.is_ok(), "rc2 fallback rejected: {result:?}");
        assert!(repaired);
        assert!(modules_repaired);
        assert_eq!(preserved, "keep");
    }

    // Opt in with a local copy of the independently hashed production payload.
    #[test]
    #[ignore = "requires independently hashed production payload; set HARMONY_FROZEN_PYTHON_TEST_PAYLOAD"]
    fn runtime_health_offline_repair_accepts_verified_frozen_previous_payload() {
        let payload = std::env::var_os("HARMONY_FROZEN_PYTHON_TEST_PAYLOAD")
            .expect("production fixture payload");
        let (root, _, destination) = repair_fixture(&[]);
        let cached_payload = root.join("installer-cache/1.1.0/payload");
        std::fs::create_dir_all(cached_payload.join("runtime")).unwrap();
        std::fs::copy(
            Path::new(&payload).join("payload.manifest.json"),
            cached_payload.join("payload.manifest.json"),
        )
        .unwrap();
        std::fs::copy(
            Path::new(&payload).join("runtime/python-runtime.zip"),
            cached_payload.join("runtime/python-runtime.zip"),
        )
        .unwrap();
        std::fs::write(destination.join("user-data.txt"), "keep").unwrap();
        assert!(!root.join("installer-cache/1.1.3-rc1").exists());
        let result = merge_cached_python_from_cache(
            &root.join("installer-cache"),
            &destination,
            "1.1.3-rc1",
        );
        let repaired = destination.join("python.exe").is_file();
        let modules_repaired = REQUIRED_PYTHON_MODULES.iter().all(|module| {
            destination
                .join(format!("Lib/site-packages/{module}/__init__.py"))
                .is_file()
        });
        let preserved = std::fs::read_to_string(destination.join("user-data.txt")).unwrap();
        #[cfg(windows)]
        {
            let output = crate::process::std_command(destination.join("python.exe"))
                .args(["-I", "-c", "import cv2,numpy,starlette,uvicorn,pydantic,imageio_ffmpeg; print('FROZEN_PYTHON_IMPORTS_OK')"])
                .output()
                .expect("launch repaired isolated Python");
            assert!(output.status.success(), "repaired Python import failed: {}", String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("FROZEN_PYTHON_IMPORTS_OK"));
        }
        for version in COMPATIBLE_PYTHON_CACHE_VERSIONS {
            set_repair_manifest(&cached_payload, version, None);
            assert!(verified_python_archive(
                &cached_payload,
                "1.1.3-rc1",
                Some(version),
                std::time::Instant::now()
            )
            .is_ok());
        }
        set_repair_manifest(&cached_payload, "0.9.0", None);
        assert!(verified_python_archive(
            &cached_payload,
            "1.1.3-rc1",
            None,
            std::time::Instant::now()
        )
        .is_err());
        set_repair_manifest(&cached_payload, "1.1.0", None);
        assert!(
            verified_python_archive(&cached_payload, "1.1.4", None, std::time::Instant::now())
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            result.is_ok(),
            "frozen previous payload rejected: {result:?}"
        );
        assert!(repaired);
        assert!(modules_repaired);
        assert_eq!(preserved, "keep");
    }

    #[test]
    fn runtime_health_offline_repair_preserves_user_files_and_rejects_bad_hash() {
        let (root, payload, destination) = repair_fixture(&[
            ("python.exe", "new interpreter"),
            ("Lib/site-packages/numpy/__init__.py", "fixed"),
        ]);
        std::fs::write(destination.join("user-data.txt"), "keep").unwrap();
        merge_cached_python(&payload, &destination, "test").unwrap();
        assert_eq!(
            std::fs::read_to_string(destination.join("user-data.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("python.exe")).unwrap(),
            "new interpreter"
        );
        std::fs::write(payload.join("runtime/python-runtime.zip"), "tampered").unwrap();
        assert!(merge_cached_python(&payload, &destination, "test")
            .unwrap_err()
            .contains("SHA-256"));
        assert!(merge_cached_python(&payload, &destination, "other")
            .unwrap_err()
            .contains("版本"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn runtime_health_offline_repair_rejects_zip_traversal_before_writing() {
        let (root, payload, destination) =
            repair_fixture(&[("python.exe", "new"), ("../escape", "bad")]);
        assert!(merge_cached_python(&payload, &destination, "test").is_err());
        assert!(!destination.join("python.exe").exists());
        assert!(!root.join("escape").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn runtime_health_offline_repair_rejects_redirected_user_directory() {
        let (root, payload, destination) = repair_fixture(&[("Lib/site-packages/cv2.py", "fixed")]);
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("Lib")).unwrap();
        assert!(merge_cached_python(&payload, &destination, "test").is_err());
        assert!(!outside.join("site-packages/cv2.py").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn runtime_health_real_local_smoke_report_has_all_required_checks() {
        let started = std::time::Instant::now();
        let report = collect(&Settings::default()).await;
        assert_eq!(report.items.len(), 12);
        assert_eq!(report.healthy, report.items.iter().all(|item| item.status == "ok"));
        assert!(started.elapsed() < Duration::from_secs(20));
        for module in REQUIRED_PYTHON_MODULES {
            assert!(report.items.iter().any(|item| item.key == format!("python:{module}")));
        }
        for key in ["node", "python", "wecom_cli", "browser", "mcp_host", "playwright_mcp"] {
            assert!(report.items.iter().any(|item| item.key == key));
        }
        println!("{}", redacted_report(&report));
    }
}
