//! Coordinator-visible pool status lines without going through the agent.

use std::sync::Arc;

use crate::agent_core::host::{PoolEvent, PoolEventSink, PoolMessageSnapshot};
use crate::pool::events::DesktopPoolEventSink;
use crate::pool::PoolMessage;
use crate::store::AppState;

fn emit_message(app: &tauri::AppHandle, msg: &PoolMessage) {
    let sink = Arc::new(DesktopPoolEventSink::new(app.clone()));
    sink.emit_pool(&PoolEvent::MessageAppended {
        pool_id: msg.pool_session_id.clone(),
        message: PoolMessageSnapshot::from(msg),
    });
}

/// Insert a Xiaoheng coordinator status line into the pool and push it to the UI.
pub async fn post_coordinator_pool_notice(
    app: &tauri::AppHandle,
    state: &AppState,
    pool_id: &str,
    content: &str,
) -> Result<PoolMessage, String> {
    let msg = {
        let db = state.db.lock().await;
        db.insert_pool_message_ext(
            pool_id,
            "xiaoheng",
            content,
            "status_update",
            r#"{"event":"coordinator_status","controlled_by":"mention_ack"}"#,
            None,
            None,
            Some("coordinator_status"),
        )
        .map_err(|e| e.to_string())?
    };
    emit_message(app, &msg);
    Ok(msg)
}
