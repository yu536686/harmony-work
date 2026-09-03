use crate::{optional_string, required_string, tool_success, HostContext};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const SUPPORTED_CHANNELS: &[&str] = &["wecom", "wechat"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Binding {
    binding_key: String,
    channel: String,
    session_id: String,
    peer_id: String,
    peer_name: Option<String>,
    latest_reply_target: String,
    routing_state_json: Option<String>,
    updated_at: String,
}

pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "im_channel_list" | "im_binding_list" | "im_binding_lookup" | "im_send_message"
    )
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"im_channel_list",
            "description":"List the two supported WorkBuddy IM transports: WeCom and personal WeChat iLink, including live desktop connection state.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false}
        }),
        json!({
            "name":"im_binding_list",
            "description":"List recent persisted WeCom or WeChat conversation bindings. Use this before sending when the exact binding key is unknown.",
            "inputSchema":{"type":"object","properties":{
                "channel":{"type":"string","enum":["wecom","wechat"]},
                "limit":{"type":"integer","minimum":1,"maximum":100,"default":20}
            },"additionalProperties":false}
        }),
        json!({
            "name":"im_binding_lookup",
            "description":"Resolve one persisted WeCom or WeChat conversation by binding_key or WorkBuddy session_id.",
            "inputSchema":{"type":"object","properties":{
                "binding_key":{"type":"string"},
                "session_id":{"type":"string"},
                "channel":{"type":"string","enum":["wecom","wechat"]}
            },"additionalProperties":false}
        }),
        json!({
            "name":"im_send_message",
            "description":"Send text and optionally one local file through the connected WorkBuddy WeCom or personal WeChat iLink channel. Prefer binding_key so reply routing state is preserved.",
            "inputSchema":{"type":"object","properties":{
                "text":{"type":"string","minLength":1},
                "binding_key":{"type":"string"},
                "channel":{"type":"string","enum":["wecom","wechat"]},
                "recipient":{"type":"string"},
                "reply_to":{"type":"string"},
                "routing_state":{},
                "file_path":{"type":"string"},
                "media_type":{"type":"string"},
                "timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}
            },"required":["text"],"additionalProperties":false}
        }),
    ]
}

pub async fn call(context: &HostContext, name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "im_channel_list" => tool_success(&list_channels(context)?),
        "im_binding_list" => {
            let channel = arguments.get("channel").and_then(Value::as_str);
            if let Some(channel) = channel {
                validate_channel(channel)?;
            }
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(20)
                .clamp(1, 100);
            tool_success(&list_bindings(context, channel, limit)?)
        }
        "im_binding_lookup" => {
            let binding_key = arguments.get("binding_key").and_then(Value::as_str);
            let session_id = arguments.get("session_id").and_then(Value::as_str);
            let channel = arguments.get("channel").and_then(Value::as_str);
            if binding_key.is_none() && session_id.is_none() {
                return Err(anyhow!("binding_key or session_id is required"));
            }
            if let Some(channel) = channel {
                validate_channel(channel)?;
            }
            tool_success(&lookup_binding(context, binding_key, session_id, channel)?)
        }
        "im_send_message" => send_message(context, arguments).await,
        _ => Err(anyhow!("unknown IM tool: {name}")),
    }
}

fn open_bridge_database(context: &HostContext) -> Result<Connection> {
    if let Some(parent) = context.database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(&context.database_path)?;
    ensure_bridge_schema(&connection)?;
    Ok(connection)
}

fn ensure_bridge_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS im_channel_state (
            channel TEXT PRIMARY KEY, status TEXT NOT NULL, detail TEXT,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS im_outbox_requests (
            id TEXT PRIMARY KEY, channel TEXT NOT NULL, recipient TEXT NOT NULL,
            content TEXT NOT NULL, session_id TEXT, reply_to TEXT, routing_state_json TEXT,
            media_path TEXT, media_type TEXT, status TEXT NOT NULL,
            error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_im_outbox_status_created
            ON im_outbox_requests(status, created_at);",
    )?;
    let _ = connection.execute(
        "ALTER TABLE im_outbox_requests ADD COLUMN session_id TEXT",
        [],
    );
    Ok(())
}

fn legacy_connection(context: &HostContext) -> Result<Connection> {
    let path = context
        .legacy_database_path
        .as_deref()
        .filter(|path| path.exists())
        .ok_or_else(|| anyhow!("conversation database is not available"))?;
    Connection::open(path).with_context(|| format!("open {}", path.display()))
}

fn list_channels(context: &HostContext) -> Result<Vec<Value>> {
    let connection = open_bridge_database(context)?;
    SUPPORTED_CHANNELS
        .iter()
        .map(|channel| {
            let state = connection
                .query_row(
                    "SELECT status, detail, updated_at FROM im_channel_state WHERE channel=?1",
                    [channel],
                    |row| {
                        Ok(json!({
                            "channel":channel,
                            "status":row.get::<_, String>(0)?,
                            "detail":row.get::<_, Option<String>>(1)?,
                            "updatedAt":row.get::<_, String>(2)?
                        }))
                    },
                )
                .optional()?;
            Ok(state.unwrap_or_else(|| {
                json!({"channel":channel,"status":"unknown","detail":"WorkBuddy desktop has not reported channel state"})
            }))
        })
        .collect()
}

fn list_bindings(context: &HostContext, channel: Option<&str>, limit: i64) -> Result<Vec<Binding>> {
    let connection = legacy_connection(context)?;
    let base = "SELECT binding_key, channel, session_id, peer_id, peer_name,
                       latest_reply_target, routing_state_json, updated_at
                FROM im_session_bindings";
    let mut output = Vec::new();
    if let Some(channel) = channel {
        let mut statement = connection.prepare(&format!(
            "{base} WHERE channel=?1 ORDER BY updated_at DESC LIMIT ?2"
        ))?;
        let rows = statement.query_map(params![channel, limit], map_binding)?;
        for row in rows {
            let binding = row?;
            if SUPPORTED_CHANNELS.contains(&binding.channel.as_str()) {
                output.push(binding);
            }
        }
    } else {
        let mut statement = connection.prepare(&format!(
            "{base} WHERE channel IN ('wecom','wechat') ORDER BY updated_at DESC LIMIT ?1"
        ))?;
        let rows = statement.query_map([limit], map_binding)?;
        for row in rows {
            output.push(row?);
        }
    }
    Ok(output)
}

fn map_binding(row: &rusqlite::Row<'_>) -> rusqlite::Result<Binding> {
    Ok(Binding {
        binding_key: row.get(0)?,
        channel: row.get(1)?,
        session_id: row.get(2)?,
        peer_id: row.get(3)?,
        peer_name: row.get(4)?,
        latest_reply_target: row.get(5)?,
        routing_state_json: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn lookup_binding(
    context: &HostContext,
    binding_key: Option<&str>,
    session_id: Option<&str>,
    channel: Option<&str>,
) -> Result<Option<Binding>> {
    let connection = legacy_connection(context)?;
    let select = "SELECT binding_key, channel, session_id, peer_id, peer_name,
                         latest_reply_target, routing_state_json, updated_at
                  FROM im_session_bindings";
    let binding = if let Some(binding_key) = binding_key {
        connection
            .query_row(
                &format!("{select} WHERE binding_key=?1 AND channel IN ('wecom','wechat')"),
                [binding_key],
                map_binding,
            )
            .optional()?
    } else if let Some(channel) = channel {
        connection
            .query_row(
                &format!(
                    "{select} WHERE session_id=?1 AND channel=?2 ORDER BY updated_at DESC LIMIT 1"
                ),
                params![session_id.unwrap_or_default(), channel],
                map_binding,
            )
            .optional()?
    } else {
        connection
            .query_row(
                &format!("{select} WHERE session_id=?1 AND channel IN ('wecom','wechat') ORDER BY updated_at DESC LIMIT 1"),
                [session_id.unwrap_or_default()],
                map_binding,
            )
            .optional()?
    };
    Ok(binding)
}

async fn send_message(context: &HostContext, arguments: &Value) -> Result<Value> {
    let content = required_string(arguments, "text")?;
    let binding_key = arguments.get("binding_key").and_then(Value::as_str);
    let explicit_channel = arguments.get("channel").and_then(Value::as_str);
    let explicit_recipient = arguments.get("recipient").and_then(Value::as_str);
    let binding = if binding_key.is_some() {
        lookup_binding(context, binding_key, None, None)?
            .ok_or_else(|| anyhow!("IM binding was not found"))?
    } else {
        let channel =
            explicit_channel.ok_or_else(|| anyhow!("channel is required without binding_key"))?;
        validate_channel(channel)?;
        Binding {
            binding_key: String::new(),
            channel: channel.to_string(),
            session_id: String::new(),
            peer_id: explicit_recipient
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("recipient is required without binding_key"))?
                .to_string(),
            peer_name: None,
            latest_reply_target: explicit_recipient.unwrap_or_default().to_string(),
            routing_state_json: arguments
                .get("routing_state")
                .filter(|value| !value.is_null())
                .map(serde_json::to_string)
                .transpose()?,
            updated_at: String::new(),
        }
    };
    validate_channel(&binding.channel)?;
    let recipient = if binding.latest_reply_target.trim().is_empty() {
        binding.peer_id.clone()
    } else {
        binding.latest_reply_target.clone()
    };
    let file_path = arguments
        .get("file_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(file_path) = file_path {
        let path = Path::new(file_path);
        if !path.is_absolute() || !path.is_file() {
            return Err(anyhow!("file_path must be an existing absolute file"));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let connection = open_bridge_database(context)?;
    connection.execute(
        "INSERT INTO im_outbox_requests (
            id, channel, recipient, content, session_id, reply_to, routing_state_json,
            media_path, media_type, status, error, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'queued',NULL,?10,?10)",
        params![
            id,
            binding.channel,
            recipient,
            content,
            binding.session_id,
            arguments.get("reply_to").and_then(Value::as_str),
            binding.routing_state_json,
            file_path,
            optional_string(arguments, "media_type", ""),
            now,
        ],
    )?;
    drop(connection);

    let timeout = arguments
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 120);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout);
    loop {
        let connection = open_bridge_database(context)?;
        let state = connection.query_row(
            "SELECT status, error FROM im_outbox_requests WHERE id=?1",
            [&id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )?;
        match state.0.as_str() {
            "delivered" => return tool_success(&json!({"requestId":id,"status":"delivered"})),
            "failed" => {
                return Err(anyhow!(state
                    .1
                    .unwrap_or_else(|| "IM delivery failed".to_string())))
            }
            _ if tokio::time::Instant::now() >= deadline => {
                return Err(anyhow!("IM delivery timed out; request_id={id}"));
            }
            _ => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

fn validate_channel(channel: &str) -> Result<()> {
    if SUPPORTED_CHANNELS.contains(&channel) {
        Ok(())
    } else {
        Err(anyhow!("only 'wecom' and 'wechat' channels are supported"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> (HostContext, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("workbuddy-im-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let legacy = root.join("legacy.db");
        let connection = Connection::open(&legacy).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE im_session_bindings (
                    binding_key TEXT PRIMARY KEY, channel TEXT NOT NULL,
                    session_id TEXT NOT NULL, peer_id TEXT NOT NULL,
                    peer_name TEXT, latest_reply_target TEXT NOT NULL,
                    routing_state_json TEXT, updated_at TEXT NOT NULL
                );
                INSERT INTO im_session_bindings VALUES (
                    'wecom::user:1','wecom','session-1','user-1','User One',
                    'reply-1','{\"req_id\":\"req-1\"}','2026-08-25T00:00:00Z'
                );",
            )
            .expect("legacy schema");
        drop(connection);
        (
            HostContext::new(
                root.clone(),
                root.join("workbuddy.db"),
                Some(legacy),
                Vec::new(),
                "http://localhost".to_string(),
            ),
            root,
        )
    }

    #[tokio::test]
    async fn resolves_binding_and_waits_for_desktop_delivery() {
        let (context, root) = context();
        let database = context.database_path.clone();
        let worker = tokio::spawn(async move {
            loop {
                let connection = Connection::open(&database).expect("bridge database");
                if ensure_bridge_schema(&connection).is_ok() {
                    let queued = connection
                        .query_row(
                            "SELECT id, recipient FROM im_outbox_requests WHERE status='queued' LIMIT 1",
                            [],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()
                        .expect("query queue");
                    if let Some((id, recipient)) = queued {
                        assert_eq!(recipient, "reply-1");
                        connection
                            .execute(
                                "UPDATE im_outbox_requests SET status='delivered' WHERE id=?1",
                                [id],
                            )
                            .expect("deliver");
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        let result = call(
            &context,
            "im_send_message",
            &json!({"text":"hello","binding_key":"wecom::user:1","timeout_seconds":2}),
        )
        .await
        .expect("send result");
        worker.await.expect("worker");
        assert_eq!(result["structuredContent"]["status"], "delivered");
        std::fs::remove_dir_all(root).expect("remove root");
    }

    #[test]
    fn rejects_channels_outside_wecom_and_wechat() {
        assert!(validate_channel("wecom").is_ok());
        assert!(validate_channel("wechat").is_ok());
        assert!(validate_channel("telegram").is_err());
    }
}
