use crate::codex_runtime::client::{
    AppServerClient, AppServerError, AppServerEvent, AppServerEventReceiver,
};
use crate::codex_runtime::config::PINNED_CODEX_VERSION;
use crate::process::tokio_command;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWrite, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;

const STDERR_HISTORY_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppServerIdentity {
    pub name: String,
    pub title: String,
    pub version: String,
}

impl AppServerIdentity {
    pub fn b1() -> Self {
        Self {
            name: "harmony-work".to_string(),
            title: "Harmony Work".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodexLaunchConfig {
    pub executable_override: Option<PathBuf>,
    pub codex_home: PathBuf,
    pub identity: AppServerIdentity,
    pub environment: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CodexProcessError {
    #[error("Codex executable was not found")]
    ExecutableNotFound,
    #[error("failed to start Codex app-server: {0}")]
    Start(#[source] std::io::Error),
    #[error("failed to inspect Codex version: {0}")]
    VersionCheck(#[source] std::io::Error),
    #[error("Codex version mismatch: expected {expected}, found {actual}")]
    VersionMismatch { expected: String, actual: String },
    #[error("Codex app-server did not expose {0}")]
    MissingPipe(&'static str),
    #[error("Codex app-server initialization failed: {0}")]
    Initialize(#[from] AppServerError),
    #[error("failed to prepare CODEX_HOME: {0}")]
    PrepareHome(#[source] std::io::Error),
    #[error("failed to stop Codex app-server: {0}")]
    Stop(#[source] std::io::Error),
}

pub struct CodexProcess {
    client: AppServerClient<ChildStdin>,
    events: Mutex<AppServerEventReceiver>,
    child: Mutex<Child>,
    stderr_history: Arc<Mutex<VecDeque<String>>>,
    executable: PathBuf,
    initialization: Value,
}

impl CodexProcess {
    pub async fn launch(config: CodexLaunchConfig) -> Result<Self, CodexProcessError> {
        std::fs::create_dir_all(&config.codex_home).map_err(CodexProcessError::PrepareHome)?;
        let executable = resolve_codex_executable(config.executable_override)?;
        verify_codex_version(&executable).await?;
        let mut command = tokio_command(&executable);
        command
            .args(app_server_args())
            .env("CODEX_HOME", &config.codex_home)
            .envs(&config.environment)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(CodexProcessError::Start)?;
        let stdin = child
            .stdin
            .take()
            .ok_or(CodexProcessError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(CodexProcessError::MissingPipe("stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(CodexProcessError::MissingPipe("stderr"))?;
        let stderr_history = Arc::new(Mutex::new(VecDeque::new()));
        capture_stderr(stderr, Arc::clone(&stderr_history));
        let (client, events) = AppServerClient::new(stdout, stdin);
        let initialization = initialize_app_server(&client, &config.identity).await?;

        Ok(Self {
            client,
            events: Mutex::new(events),
            child: Mutex::new(child),
            stderr_history,
            executable,
            initialization,
        })
    }

    pub fn client(&self) -> AppServerClient<ChildStdin> {
        self.client.clone()
    }

    pub async fn next_event(&self) -> Option<AppServerEvent> {
        self.events.lock().await.recv().await
    }

    pub async fn stderr_snapshot(&self) -> Vec<String> {
        self.stderr_history.lock().await.iter().cloned().collect()
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn initialization(&self) -> &Value {
        &self.initialization
    }

    pub async fn is_running(&self) -> Result<bool, CodexProcessError> {
        self.child
            .lock()
            .await
            .try_wait()
            .map(|status| status.is_none())
            .map_err(CodexProcessError::Stop)
    }

    pub async fn shutdown(&self) -> Result<(), CodexProcessError> {
        let mut child = self.child.lock().await;
        if child.try_wait().map_err(CodexProcessError::Stop)?.is_none() {
            child.start_kill().map_err(CodexProcessError::Stop)?;
            child.wait().await.map_err(CodexProcessError::Stop)?;
        }
        Ok(())
    }
}

pub async fn initialize_app_server<W>(
    client: &AppServerClient<W>,
    identity: &AppServerIdentity,
) -> Result<Value, AppServerError>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let result = client
        .request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": identity.name,
                    "title": identity.title,
                    "version": identity.version,
                }
            }),
        )
        .await?;
    client.notify("initialized", json!({})).await?;
    Ok(result)
}

pub const fn app_server_args() -> [&'static str; 3] {
    ["app-server", "--listen", "stdio://"]
}

pub fn resolve_codex_executable(
    executable_override: Option<PathBuf>,
) -> Result<PathBuf, CodexProcessError> {
    if let Some(path) = executable_override {
        return path
            .is_file()
            .then_some(path)
            .ok_or(CodexProcessError::ExecutableNotFound);
    }

    if let Some(path) = std::env::var_os("CODEX_PATH").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    for path in bundled_codex_candidates() {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Some(path) = npm_codex_candidate() {
        if path.is_file() {
            return Ok(path);
        }
    }

    find_in_path().ok_or(CodexProcessError::ExecutableNotFound)
}

fn bundled_codex_candidates() -> Vec<PathBuf> {
    let Ok(current_exe) = std::env::current_exe() else {
        return Vec::new();
    };
    bundled_codex_candidates_from(&current_exe)
}

fn bundled_codex_candidates_from(current_exe: &Path) -> Vec<PathBuf> {
    let Some(directory) = current_exe.parent() else {
        return Vec::new();
    };
    let binary = codex_binary_name();
    let mut candidates = vec![
        directory.join(binary),
        directory.join("resources").join(binary),
        directory.join("resources").join("codex").join(binary),
    ];
    if let Some(contents) = directory.parent() {
        candidates.push(contents.join("Resources").join(binary));
        candidates.push(
            contents
                .join("Resources")
                .join("resources")
                .join("codex")
                .join(binary),
        );
    }
    candidates
}

async fn verify_codex_version(executable: &Path) -> Result<(), CodexProcessError> {
    let output = tokio_command(executable)
        .arg("--version")
        .output()
        .await
        .map_err(CodexProcessError::VersionCheck)?;
    let actual = parse_codex_version(&String::from_utf8_lossy(&output.stdout))
        .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
    if !output.status.success() || actual != PINNED_CODEX_VERSION {
        return Err(CodexProcessError::VersionMismatch {
            expected: PINNED_CODEX_VERSION.to_string(),
            actual: if actual.is_empty() {
                "unknown".to_string()
            } else {
                actual
            },
        });
    }
    Ok(())
}

fn parse_codex_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|part| {
            part.chars()
                .next()
                .is_some_and(|value| value.is_ascii_digit())
        })
        .map(|part| part.trim().to_string())
}

fn npm_codex_candidate() -> Option<PathBuf> {
    let (package_suffix, target_triple) = npm_codex_target()?;
    let current_dir = std::env::current_dir().ok()?;
    Some(
        current_dir
            .join("node_modules")
            .join("@openai")
            .join(format!("codex-{package_suffix}"))
            .join("vendor")
            .join(target_triple)
            .join("bin")
            .join(codex_binary_name()),
    )
}

fn npm_codex_target() -> Option<(&'static str, &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some(("darwin-arm64", "aarch64-apple-darwin")),
        ("macos", "x86_64") => Some(("darwin-x64", "x86_64-apple-darwin")),
        ("windows", "x86_64") => Some(("win32-x64", "x86_64-pc-windows-msvc")),
        ("windows", "aarch64") => Some(("win32-arm64", "aarch64-pc-windows-msvc")),
        ("linux", "x86_64") => Some(("linux-x64", "x86_64-unknown-linux-musl")),
        ("linux", "aarch64") => Some(("linux-arm64", "aarch64-unknown-linux-musl")),
        _ => None,
    }
}

fn find_in_path() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(codex_binary_name()))
            .find(|candidate| candidate.is_file())
    })
}

const fn codex_binary_name() -> &'static str {
    if cfg!(windows) {
        "codex.exe"
    } else {
        "codex"
    }
}

fn capture_stderr(stderr: tokio::process::ChildStderr, history: Arc<Mutex<VecDeque<String>>>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut history = history.lock().await;
            if history.len() == STDERR_HISTORY_LIMIT {
                history.pop_front();
            }
            history.push_back(line);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{split, AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn initialize_uses_required_handshake_order() {
        let (client_stream, server_stream) = tokio::io::duplex(4096);
        let (client_reader, client_writer) = split(client_stream);
        let (server_reader, mut server_writer) = split(server_stream);
        let (client, _events) = AppServerClient::new(client_reader, client_writer);
        let server = tokio::spawn(async move {
            let mut lines = BufReader::new(server_reader).lines();
            let request: serde_json::Value =
                serde_json::from_str(&lines.next_line().await.expect("read").expect("initialize"))
                    .expect("json");
            assert_eq!(request["method"], "initialize");
            assert_eq!(
                request["params"]["clientInfo"]["name"],
                "harmony-work"
            );
            server_writer
                .write_all(
                    format!(
                        "{}\n",
                        json!({"id": request["id"], "result": {"userAgent": "codex-test"}})
                    )
                    .as_bytes(),
                )
                .await
                .expect("respond");

            let notification: serde_json::Value =
                serde_json::from_str(&lines.next_line().await.expect("read").expect("initialized"))
                    .expect("json");
            assert_eq!(notification["method"], "initialized");
        });

        let result = initialize_app_server(&client, &AppServerIdentity::b1())
            .await
            .expect("handshake");
        assert_eq!(result["userAgent"], "codex-test");
        server.await.expect("server task");
    }

    #[test]
    fn launch_arguments_select_stdio_transport() {
        assert_eq!(app_server_args(), ["app-server", "--listen", "stdio://"]);
    }

    #[test]
    fn explicit_executable_path_has_priority() {
        let path = std::env::temp_dir().join(format!("codex-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"test").expect("create executable fixture");

        let resolved = resolve_codex_executable(Some(path.clone())).expect("resolve override");
        assert_eq!(resolved, path);

        std::fs::remove_file(resolved).expect("remove fixture");
    }

    #[test]
    fn bundled_candidates_include_tauri_resource_layout() {
        let executable = Path::new("/Applications/WorkBuddy.app/Contents/MacOS/workbuddy");
        let candidates = bundled_codex_candidates_from(executable);
        let expected = Path::new("/Applications/WorkBuddy.app/Contents/Resources")
            .join("resources")
            .join("codex")
            .join(codex_binary_name());
        assert!(candidates.contains(&expected));
    }

    #[test]
    fn parses_pinned_codex_cli_version() {
        assert_eq!(
            parse_codex_version("codex-cli 0.149.0\n").as_deref(),
            Some("0.149.0")
        );
        assert_eq!(parse_codex_version("unexpected output"), None);
    }
}
