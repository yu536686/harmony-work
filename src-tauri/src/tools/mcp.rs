//! MCP (Model Context Protocol) client and tool proxy.
//!
//! Supports three transports:
//!   - stdio: spawn a local process and communicate via stdin/stdout JSON-RPC
//!   - sse:   connect to an HTTP server via Server-Sent Events
//!   - http:  streamable HTTP — POST JSON-RPC to a single endpoint, reading a
//!     JSON or `text/event-stream` response (MCP 2025 transport).
//!
//! Both `sse` and `http` honor `McpServerConfig::headers` (e.g. Bearer auth),
//! enabling authenticated remote servers / connectors.
//!
//! Each MCP server exposes a list of tools; each tool is registered as a
//! separate `McpProxyTool` in the tool registry.

pub use crate::store::settings::McpServerConfig;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::info;

// ─── Config ──────────────────────────────────────────────────────────────────
//
// The serialized `McpServerConfig` lives with the desktop Settings schema and
// is re-exported here for the settings commands.

// ─── Tool info returned by tools/list ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Option<Value>,
}

// ─── JSON-RPC helpers ─────────────────────────────────────────────────────────

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

fn make_request(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": next_id(),
        "method": method,
        "params": params,
    })
}

// ─── Stdio transport ──────────────────────────────────────────────────────────

struct StdioTransport {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _child: Child,
}

impl StdioTransport {
    async fn spawn(config: &McpServerConfig) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(&config.command);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.as_std_mut().creation_flags(0x08000000);
        }
        cmd.args(&config.args)
            .envs(&config.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn MCP server '{}': {}", config.command, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdin for MCP server"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("Failed to get stdout for MCP server"))?;

        Ok(Self {
            stdin,
            stdout: BufReader::new(stdout),
            _child: child,
        })
    }

    async fn send_request(&mut self, request: &Value) -> Result<Value> {
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        let mut response_line = String::new();
        self.stdout.read_line(&mut response_line).await?;
        let response: Value = serde_json::from_str(response_line.trim()).map_err(|e| {
            anyhow!(
                "Invalid JSON-RPC response: {} (raw: {})",
                e,
                response_line.trim()
            )
        })?;
        Ok(response)
    }
}

// ─── Header helpers ───────────────────────────────────────────────────────────

/// Apply caller-configured HTTP headers (e.g. `Authorization: Bearer …`) from
/// `McpServerConfig::headers` onto an outgoing request.
fn apply_headers(
    mut req: reqwest::RequestBuilder,
    headers: &HashMap<String, String>,
) -> reqwest::RequestBuilder {
    for (k, v) in headers {
        if !k.is_empty() {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    req
}

// ─── SSE transport ────────────────────────────────────────────────────────────

struct SseTransport {
    base_url: String,
    client: reqwest::Client,
    endpoint: Option<String>,
    headers: HashMap<String, String>,
}

impl SseTransport {
    fn new(url: &str, headers: HashMap<String, String>) -> Self {
        Self {
            base_url: url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            endpoint: None,
            headers,
        }
    }

    async fn connect(&mut self) -> Result<()> {
        // Connect to SSE stream to get the endpoint URL
        let sse_url = format!("{}/sse", self.base_url);
        let req = self
            .client
            .get(&sse_url)
            .header("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(10));
        let resp = apply_headers(req, &self.headers)
            .send()
            .await
            .map_err(|e| anyhow!("SSE connect failed: {}", e))?;

        let text = resp.text().await?;
        // Parse "data: /message?sessionId=..." from SSE stream
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                let endpoint = if data.starts_with('/') {
                    format!("{}{}", self.base_url, data)
                } else {
                    data.to_string()
                };
                self.endpoint = Some(endpoint);
                break;
            }
        }
        if self.endpoint.is_none() {
            // Fallback: use /message endpoint directly
            self.endpoint = Some(format!("{}/message", self.base_url));
        }
        Ok(())
    }

    async fn send_request(&self, request: &Value) -> Result<Value> {
        let fallback = format!("{}/message", self.base_url);
        let endpoint = self.endpoint.as_deref().unwrap_or(&fallback);

        let req = self
            .client
            .post(endpoint)
            .json(request)
            .timeout(std::time::Duration::from_secs(30));
        let resp = apply_headers(req, &self.headers)
            .send()
            .await
            .map_err(|e| anyhow!("SSE request failed: {}", e))?;

        let response: Value = resp
            .json()
            .await
            .map_err(|e| anyhow!("Invalid JSON-RPC response: {}", e))?;
        Ok(response)
    }
}

// ─── Streamable HTTP transport ─────────────────────────────────────────────────
//
// MCP "streamable HTTP" transport: a single endpoint URL accepts JSON-RPC over
// POST. The server responds with either `application/json` (one JSON-RPC
// message) or `text/event-stream` (one or more SSE `data:` frames). An optional
// `Mcp-Session-Id` response header is echoed back on subsequent requests.

struct StreamableHttpTransport {
    url: String,
    client: reqwest::Client,
    headers: HashMap<String, String>,
    session_id: Option<String>,
}

impl StreamableHttpTransport {
    fn new(url: &str, headers: HashMap<String, String>) -> Self {
        Self {
            url: url.to_string(),
            client: reqwest::Client::new(),
            headers,
            session_id: None,
        }
    }

    /// Send a JSON-RPC request (or notification) and return the parsed response.
    /// Notifications (no `id`) still POST but their body may be empty.
    async fn send_request(&mut self, request: &Value) -> Result<Value> {
        let req = self
            .client
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .json(request)
            .timeout(std::time::Duration::from_secs(60));
        let mut req = apply_headers(req, &self.headers);
        if let Some(sid) = &self.session_id {
            req = req.header("Mcp-Session-Id", sid.as_str());
        }

        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("HTTP MCP request failed: {}", e))?;

        // Capture the session id (set during `initialize`) for later requests.
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            self.session_id = Some(sid.to_string());
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp
            .text()
            .await
            .map_err(|e| anyhow!("HTTP MCP read failed: {}", e))?;

        parse_jsonrpc_body(&content_type, &body)
    }
}

/// Parse a JSON-RPC response from a streamable-HTTP body. Handles both a bare
/// JSON object and an SSE `text/event-stream` payload (returning the first
/// `data:` frame that carries a JSON-RPC `result`/`error`).
fn parse_jsonrpc_body(content_type: &str, body: &str) -> Result<Value> {
    let looks_sse = content_type.contains("text/event-stream")
        || body.trim_start().starts_with("event:")
        || body.contains("\ndata:")
        || body.starts_with("data:");
    if looks_sse {
        let mut last: Option<Value> = None;
        for line in body.lines() {
            let data = line
                .strip_prefix("data:")
                .map(|s| s.trim_start())
                .filter(|s| !s.is_empty());
            if let Some(payload) = data {
                if let Ok(v) = serde_json::from_str::<Value>(payload) {
                    if v.get("result").is_some() || v.get("error").is_some() {
                        return Ok(v);
                    }
                    last = Some(v);
                }
            }
        }
        if let Some(v) = last {
            return Ok(v);
        }
        // A notification or empty stream — synthesize an empty ok response.
        if body.trim().is_empty() {
            return Ok(json!({"jsonrpc": "2.0", "result": {}}));
        }
        return Err(anyhow!("HTTP MCP: no JSON-RPC frame in SSE body"));
    }

    if body.trim().is_empty() {
        // Accepted notification with no content.
        return Ok(json!({"jsonrpc": "2.0", "result": {}}));
    }
    serde_json::from_str::<Value>(body)
        .map_err(|e| anyhow!("Invalid JSON-RPC response: {} (raw: {})", e, body))
}

// ─── MCP Client ───────────────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
enum Transport {
    Stdio(StdioTransport),
    Sse(SseTransport),
    Http(StreamableHttpTransport),
}

pub struct McpClient {
    config: McpServerConfig,
    transport: Mutex<Option<Transport>>,
}

impl McpClient {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            transport: Mutex::new(None),
        }
    }

    async fn ensure_connected(&self) -> Result<()> {
        let mut guard = self.transport.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let transport = match self.config.transport.as_str() {
            "stdio" => {
                let mut t = StdioTransport::spawn(&self.config).await?;
                // Initialize handshake
                let init_req = make_request(
                    "initialize",
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "workbuddy-desktop", "version": "1.0.0" }
                    }),
                );
                let resp = t.send_request(&init_req).await?;
                if resp.get("error").is_some() {
                    return Err(anyhow!("MCP initialize error: {}", resp["error"]));
                }
                // Send initialized notification
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                });
                let mut line = serde_json::to_string(&notif)?;
                line.push('\n');
                t.stdin.write_all(line.as_bytes()).await?;
                t.stdin.flush().await?;

                Transport::Stdio(t)
            }
            "sse" => {
                let mut t = SseTransport::new(&self.config.url, self.config.headers.clone());
                t.connect().await?;
                // Initialize handshake
                let init_req = make_request(
                    "initialize",
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "workbuddy-desktop", "version": "1.0.0" }
                    }),
                );
                let resp = t.send_request(&init_req).await?;
                if resp.get("error").is_some() {
                    return Err(anyhow!("MCP initialize error: {}", resp["error"]));
                }
                Transport::Sse(t)
            }
            "http" | "streamable-http" | "streamable_http" => {
                let mut t =
                    StreamableHttpTransport::new(&self.config.url, self.config.headers.clone());
                // Initialize handshake (also captures the Mcp-Session-Id header).
                let init_req = make_request(
                    "initialize",
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": { "name": "workbuddy-desktop", "version": "1.0.0" }
                    }),
                );
                let resp = t.send_request(&init_req).await?;
                if resp.get("error").is_some() {
                    return Err(anyhow!("MCP initialize error: {}", resp["error"]));
                }
                // Send initialized notification so the server marks the session ready.
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {}
                });
                let _ = t.send_request(&notif).await;
                Transport::Http(t)
            }
            other => return Err(anyhow!("Unknown MCP transport: {}", other)),
        };

        *guard = Some(transport);
        info!("MCP server '{}' connected", self.config.name);
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.ensure_connected().await?;
        let req = make_request("tools/list", json!({}));
        let resp = self.send_rpc(req).await?;
        let tools = resp["result"]["tools"]
            .as_array()
            .ok_or_else(|| anyhow!("tools/list: missing tools array"))?;
        let infos: Vec<McpToolInfo> = tools
            .iter()
            .filter_map(|t| serde_json::from_value(t.clone()).ok())
            .collect();
        Ok(infos)
    }

    async fn send_rpc(&self, req: Value) -> Result<Value> {
        let mut guard = self.transport.lock().await;
        match guard.as_mut() {
            Some(Transport::Stdio(t)) => t.send_request(&req).await,
            Some(Transport::Sse(t)) => t.send_request(&req).await,
            Some(Transport::Http(t)) => t.send_request(&req).await,
            None => Err(anyhow!("MCP transport not connected")),
        }
    }
}
