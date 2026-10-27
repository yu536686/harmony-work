//! Local global search. Parent registers this module/command; never calls an LLM.
#[path = "global_search/query.rs"]
mod query;

use crate::commands::mobile_bridge::current_device_auth;
use crate::store::{db::Session, AppState};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub source: String,
    pub session: Option<Session>,
    pub artifact_id: Option<String>,
    pub workspace_root: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchResponse {
    pub results: Vec<GlobalSearchItem>,
    pub has_more: bool,
}

#[tauri::command]
pub async fn global_search(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<GlobalSearchResponse, String> {
    let auth = current_device_auth();
    let workspace = state.settings.lock().await.workspace_root.clone();
    let db = state.db.lock().await;
    if auth != current_device_auth() {
        return Err("ACCOUNT_CHANGED: repeat search for the current account".into());
    }
    // The legacy mobile table has no account ownership column. Authentication
    // alone is not proof of a row's owner, so omit these rows until that contract exists.
    let (hits, has_more) = query::search_hits(
        &db.conn,
        &query,
        limit.unwrap_or(32),
        false,
        &workspace,
    )
    .map_err(|e| e.to_string())?;
    let mut results = Vec::with_capacity(hits.len());
    for hit in hits {
        let session = match &hit.session_id {
            Some(id) => db.get_session(id).map_err(|e| e.to_string())?,
            None => None,
        };
        if hit.session_id.is_some() && session.is_none() {
            continue;
        }
        let id = match hit.kind.as_str() {
            "artifact" => format!(
                "artifact:{}:{}",
                hit.session_id.as_deref().unwrap_or(""),
                hit.artifact_id.as_deref().unwrap_or("")
            ),
            "workspace" => format!("workspace:{}", hit.workspace_root.as_deref().unwrap_or("")),
            _ => format!("session:{}", hit.session_id.as_deref().unwrap_or("")),
        };
        results.push(GlobalSearchItem {
            id,
            kind: hit.kind,
            title: hit.title,
            source: hit.source,
            session,
            artifact_id: hit.artifact_id,
            workspace_root: hit.workspace_root,
        });
    }
    if auth != current_device_auth() {
        return Err("ACCOUNT_CHANGED: repeat search for the current account".into());
    }
    Ok(GlobalSearchResponse { results, has_more })
}
