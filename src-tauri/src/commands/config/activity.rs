use crate::store::{
    db::{AuditEntry, PlanSnapshot, Session, SessionArtifact, SkillRevision},
    AppState,
};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct SessionActivityBundle {
    pub session_id: String,
    pub session_title: String,
    pub session_updated_at: String,
    pub audits: Vec<AuditEntry>,
    pub plan_snapshots: Vec<PlanSnapshot>,
    pub artifacts: Vec<SessionArtifact>,
    pub skill_revisions: Vec<SkillRevision>,
}

fn is_internal_activity_session(session_id: &str, session: Option<&Session>) -> bool {
    let source = session.map(|item| item.source.as_str()).unwrap_or_default();
    let title = session
        .and_then(|item| item.title.as_deref())
        .unwrap_or_default();
    matches!(
        source,
        "heartbeat"
            | "heartbeat_pool"
            | "codex_heartbeat_global"
            | "codex_heartbeat_pool"
            | "piscis_heartbeat_global"
            | "piscis_heartbeat_pool"
            | "piscis_internal"
    ) || matches!(
        session_id,
        "heartbeat" | "workbuddy_heartbeat_global" | "piscis_heartbeat_global"
    ) || (source == "scheduled_task" && title == "[定时] Memory Consolidation")
}

#[tauri::command]
pub async fn get_session_activity_log(
    state: State<'_, AppState>,
    limit_sessions: Option<i64>,
) -> Result<Vec<SessionActivityBundle>, String> {
    let limit = limit_sessions.unwrap_or(30).clamp(1, 100);
    let db = state.db.lock().await;
    let session_ids = db
        .list_activity_session_ids(limit)
        .map_err(|e| e.to_string())?;

    let mut bundles = Vec::with_capacity(session_ids.len());
    for session_id in session_ids {
        let session: Option<Session> = db.get_session(&session_id).map_err(|e| e.to_string())?;
        if is_internal_activity_session(&session_id, session.as_ref()) {
            continue;
        }
        let session_title = session
            .as_ref()
            .and_then(|s| s.title.clone())
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| session_id.clone());
        let session_updated_at = session
            .as_ref()
            .map(|s| s.updated_at.to_rfc3339())
            .unwrap_or_default();

        let audits = db
            .get_audit_log(Some(&session_id), None, 200, 0)
            .unwrap_or_default();
        let plan_snapshots = db
            .list_plan_snapshots_for_session(&session_id, 50)
            .unwrap_or_default();
        let artifacts = db
            .list_session_artifacts(&session_id, 50)
            .unwrap_or_default();
        let skill_revisions = db
            .list_skill_revisions_for_session(&session_id, 30)
            .unwrap_or_default();

        if audits.is_empty()
            && plan_snapshots.is_empty()
            && artifacts.is_empty()
            && skill_revisions.is_empty()
        {
            continue;
        }

        bundles.push(SessionActivityBundle {
            session_id,
            session_title,
            session_updated_at,
            audits,
            plan_snapshots,
            artifacts,
            skill_revisions,
        });
    }
    Ok(bundles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn session(id: &str, title: &str, source: &str) -> Session {
        Session {
            id: id.to_string(),
            title: Some(title.to_string()),
            source: source.to_string(),
            status: "idle".to_string(),
            rolling_summary: String::new(),
            rolling_summary_version: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            last_compacted_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            message_count: 0,
            workspace_root: None,
            pinned_at: None,
            archived_at: None,
            pool_session_id: None,
            policy_mode: None,
            allow_outside_workspace: None,
        }
    }

    #[test]
    fn hides_current_and_legacy_heartbeat_activity() {
        let current = session(
            "workbuddy_heartbeat_global",
            "小亨后台巡检",
            "codex_heartbeat_global",
        );
        let legacy = session(
            "piscis_heartbeat_global",
            "Piscis Heartbeat",
            "piscis_heartbeat_global",
        );
        assert!(is_internal_activity_session(&current.id, Some(&current)));
        assert!(is_internal_activity_session(&legacy.id, Some(&legacy)));
    }

    #[test]
    fn hides_internal_memory_consolidation_but_keeps_user_scheduled_tasks() {
        let consolidation = session(
            "sched-memory",
            "[定时] Memory Consolidation",
            "scheduled_task",
        );
        let user_task = session("sched-report", "[定时] 周报", "scheduled_task");
        assert!(is_internal_activity_session(
            &consolidation.id,
            Some(&consolidation)
        ));
        assert!(!is_internal_activity_session(
            &user_task.id,
            Some(&user_task)
        ));
    }
}
