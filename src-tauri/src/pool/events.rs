use piscis_core::host::{PoolEvent, PoolEventSink};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub const POOL_EVENT_CANONICAL_CHANNEL: &str = "host://pool_event";

#[derive(Clone)]
pub struct DesktopPoolEventSink {
    app: AppHandle,
}

impl DesktopPoolEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

pub fn pool_event_envelopes(event: &PoolEvent) -> Vec<(String, Value)> {
    match event {
        PoolEvent::PoolCreated { pool } => vec![(
            "pool_session_created".into(),
            serde_json::to_value(pool).unwrap_or(Value::Null),
        )],
        PoolEvent::PoolUpdated { pool }
        | PoolEvent::PoolPaused { pool }
        | PoolEvent::PoolResumed { pool } => vec![(
            "pool_session_updated".into(),
            serde_json::to_value(pool).unwrap_or(Value::Null),
        )],
        PoolEvent::PoolArchived { pool_id } => vec![(
            "pool_session_updated".into(),
            json!({"id":pool_id,"status":"archived"}),
        )],
        PoolEvent::MessageAppended { pool_id, message } => vec![(
            format!("pool_message_{pool_id}"),
            serde_json::to_value(message).unwrap_or(Value::Null),
        )],
        PoolEvent::TodoChanged {
            pool_id,
            action,
            todo,
        } => vec![(
            "koi_todo_updated".into(),
            json!({"id":todo.id,"pool_id":pool_id,"action":action,"todo":todo}),
        )],
        PoolEvent::KoiAssigned {
            pool_id,
            koi_id,
            todo_id,
        } => vec![(
            "koi_status_changed".into(),
            json!({"id":koi_id,"pool_id":pool_id,"status":"assigned","todo_id":todo_id}),
        )],
        PoolEvent::KoiStatusChanged {
            pool_id,
            koi_id,
            status,
        } => vec![(
            "koi_status_changed".into(),
            json!({"id":koi_id,"pool_id":pool_id,"status":status}),
        )],
        PoolEvent::KoiStaleRecovered {
            pool_id,
            koi_id,
            recovered_todo_count,
        } => vec![(
            "koi_status_changed".into(),
            json!({"id":koi_id,"pool_id":pool_id,"status":"idle","recovered_todo_count":recovered_todo_count,"stale_recovered":true}),
        )],
        PoolEvent::CoordinatorIdle { pool_id } => vec![(
            "pool_coordinator_idle".into(),
            json!({"pool_id":pool_id}),
        )],
        PoolEvent::CoordinatorCompleted { pool_id, summary } => vec![(
            "pool_coordinator_completed".into(),
            json!({"pool_id":pool_id,"summary":summary}),
        )],
        PoolEvent::CoordinatorTimedOut { pool_id, summary } => vec![(
            "pool_coordinator_timed_out".into(),
            json!({"pool_id":pool_id,"summary":summary}),
        )],
        PoolEvent::FishProgress {
            parent_session_id,
            fish_id,
            stage,
            payload,
        } => vec![(
            format!("fish_progress_{parent_session_id}"),
            json!({"fish_id":fish_id,"stage":stage,"payload":payload}),
        )],
    }
}

impl PoolEventSink for DesktopPoolEventSink {
    fn emit_pool(&self, event: &PoolEvent) {
        let _ = self.app.emit(POOL_EVENT_CANONICAL_CHANNEL, event);
        for (name, payload) in pool_event_envelopes(event) {
            let _ = self.app.emit(&name, payload);
        }
    }
}
