use crate::gateway::{ChannelStatus, MediaAttachment, OutboundMessage};
use crate::store::AppState;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct OutboxRequest {
    id: String,
    channel: String,
    recipient: String,
    content: String,
    session_id: Option<String>,
    reply_to: Option<String>,
    routing_state_json: Option<String>,
    media_path: Option<String>,
    media_type: Option<String>,
}

pub fn spawn_im_bridge_worker(state: AppState, app_data_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let database_path = app_data_dir.join("workbuddy.db");
        loop {
            if let Err(error) = process_bridge_tick(&state, &database_path).await {
                tracing::warn!("WorkBuddy IM bridge tick failed: {error}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    });
}

async fn process_bridge_tick(state: &AppState, database_path: &Path) -> Result<(), String> {
    let channels = state.gateway.list_channels().await;
    let connection = open_bridge_database(database_path)?;
    let now = Utc::now().to_rfc3339();
    for channel in channels
        .into_iter()
        .filter(|channel| matches!(channel.name.as_str(), "wecom" | "wechat"))
    {
        let (status, detail) = channel_status(&channel.status);
        connection
            .execute(
                "INSERT INTO im_channel_state(channel,status,detail,updated_at)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(channel) DO UPDATE SET status=excluded.status,
                    detail=excluded.detail,updated_at=excluded.updated_at",
                params![channel.name, status, detail, now],
            )
            .map_err(|error| error.to_string())?;
    }
    let request = claim_next_request(&connection)?;
    drop(connection);
    if let Some(request) = request {
        deliver_request(state, database_path, request).await?;
    }
    Ok(())
}

fn open_bridge_database(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let connection = Connection::open(path).map_err(|error| error.to_string())?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS im_channel_state (
                channel TEXT PRIMARY KEY, status TEXT NOT NULL, detail TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS im_outbox_requests (
                id TEXT PRIMARY KEY, channel TEXT NOT NULL, recipient TEXT NOT NULL,
                content TEXT NOT NULL, session_id TEXT, reply_to TEXT,
                routing_state_json TEXT, media_path TEXT, media_type TEXT,
                status TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_im_outbox_status_created
                ON im_outbox_requests(status, created_at);",
        )
        .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn claim_next_request(connection: &Connection) -> Result<Option<OutboxRequest>, String> {
    let request = connection
        .query_row(
            "SELECT id,channel,recipient,content,session_id,reply_to,routing_state_json,
                    media_path,media_type
             FROM im_outbox_requests WHERE status='queued'
             ORDER BY created_at LIMIT 1",
            [],
            |row| {
                Ok(OutboxRequest {
                    id: row.get(0)?,
                    channel: row.get(1)?,
                    recipient: row.get(2)?,
                    content: row.get(3)?,
                    session_id: row.get(4)?,
                    reply_to: row.get(5)?,
                    routing_state_json: row.get(6)?,
                    media_path: row.get(7)?,
                    media_type: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(request) = request else {
        return Ok(None);
    };
    let changed = connection
        .execute(
            "UPDATE im_outbox_requests SET status='sending',updated_at=?2
             WHERE id=?1 AND status='queued'",
            params![request.id, Utc::now().to_rfc3339()],
        )
        .map_err(|error| error.to_string())?;
    Ok((changed == 1).then_some(request))
}

async fn deliver_request(
    state: &AppState,
    database_path: &Path,
    request: OutboxRequest,
) -> Result<(), String> {
    let media = request
        .media_path
        .as_deref()
        .map(|path| load_media(path, request.media_type.as_deref()))
        .transpose();
    let routing_state = request
        .routing_state_json
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|error| error.to_string());
    let outcome = match (media, routing_state) {
        (Ok(media), Ok(routing_state)) => state
            .gateway
            .send(&OutboundMessage {
                channel: request.channel.clone(),
                recipient: request.recipient.clone(),
                content: request.content.clone(),
                reply_to: request.reply_to.clone(),
                media,
                routing_state,
            })
            .await
            .map_err(|error| error.to_string()),
        (Err(error), _) | (_, Err(error)) => Err(error),
    };
    let connection = open_bridge_database(database_path)?;
    match outcome {
        Ok(()) => {
            connection
                .execute(
                    "UPDATE im_outbox_requests SET status='delivered',error=NULL,updated_at=?2 WHERE id=?1",
                    params![request.id, Utc::now().to_rfc3339()],
                )
                .map_err(|error| error.to_string())?;
            if let Some(session_id) = request
                .session_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let db = state.db.lock().await;
                let _ = db.append_message(session_id, "assistant", &request.content);
            }
        }
        Err(error) => {
            connection
                .execute(
                    "UPDATE im_outbox_requests SET status='failed',error=?2,updated_at=?3 WHERE id=?1",
                    params![request.id, error, Utc::now().to_rfc3339()],
                )
                .map_err(|database_error| database_error.to_string())?;
        }
    }
    Ok(())
}

fn load_media(path: &str, media_type: Option<&str>) -> Result<MediaAttachment, String> {
    let path = Path::new(path);
    let data = std::fs::read(path).map_err(|error| error.to_string())?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment")
        .to_string();
    Ok(MediaAttachment {
        media_type: media_type
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| mime_from_path(path)),
        url: None,
        data: Some(data),
        filename: Some(filename),
    })
}

fn mime_from_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn channel_status(status: &ChannelStatus) -> (&'static str, Option<String>) {
    match status {
        ChannelStatus::Disconnected => ("disconnected", None),
        ChannelStatus::Connecting => ("connecting", None),
        ChannelStatus::Connected => ("connected", None),
        ChannelStatus::Error(error) => ("error", Some(error.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_each_outbox_request_once() {
        let path = std::env::temp_dir().join(format!("im-bridge-{}.db", uuid::Uuid::new_v4()));
        let connection = open_bridge_database(&path).expect("database");
        let now = Utc::now().to_rfc3339();
        connection
            .execute(
                "INSERT INTO im_outbox_requests (
                    id,channel,recipient,content,status,created_at,updated_at
                 ) VALUES ('request-1','wecom','user-1','hello','queued',?1,?1)",
                [now],
            )
            .expect("queue request");
        assert_eq!(
            claim_next_request(&connection).unwrap().unwrap().id,
            "request-1"
        );
        assert!(claim_next_request(&connection).unwrap().is_none());
        drop(connection);
        std::fs::remove_file(path).expect("remove database");
    }

    #[test]
    fn limits_channel_status_to_serializable_values() {
        assert_eq!(channel_status(&ChannelStatus::Connected).0, "connected");
        assert_eq!(
            channel_status(&ChannelStatus::Error("bad".into()))
                .1
                .as_deref(),
            Some("bad")
        );
    }
}
