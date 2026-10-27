//! Read-only bounded local search. No messages, FTS or remote providers.
use anyhow::{ensure, Result};
use rusqlite::{params, Connection};

pub const MAX_QUERY_CHARS: usize = 128;
pub const MAX_RESULTS: usize = 40;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub kind: String,
    pub title: String,
    pub session_id: Option<String>,
    pub artifact_id: Option<String>,
    pub workspace_root: Option<String>,
    pub source: String,
}

const VISIBLE: &str = "s.archived_at IS NULL
    AND lower(COALESCE(s.source,'chat')) NOT IN (
      'heartbeat','heartbeat_pool','codex_heartbeat_global','codex_heartbeat_pool',
      'codex_staffdeck','piscis_inbox_global','piscis_inbox_pool','piscis_internal',
      'piscis_pool','piscis_heartbeat_global','piscis_heartbeat_pool','mobile_executor')
    AND lower(s.id) NOT IN ('heartbeat','workbuddy_heartbeat_global','piscis_inbox_global')
    AND substr(lower(s.id),1,14) != 'xiaoheng_pool_'
    AND substr(lower(s.id),1,12) != 'piscis_pool_'
    AND substr(lower(s.id),1,4) != 'koi_'
    AND (substr(lower(COALESCE(s.source,'chat')),1,6) != 'mobile'
         OR (?2=1 AND s.source='mobile'))";

pub fn search_hits(
    conn: &Connection,
    query: &str,
    limit: usize,
    include_mobile: bool,
    default_workspace: &str,
) -> Result<(Vec<SearchHit>, bool)> {
    ensure!(
        query.chars().count() <= MAX_QUERY_CHARS && !query.contains('\0'),
        "INVALID_QUERY: maximum 128 characters and no NUL bytes"
    );
    let query = query.trim();
    let limit = limit.clamp(1, MAX_RESULTS);
    let per_kind = (limit + 1) as i64;
    let mut sessions = conn.prepare(&format!("SELECT s.id,COALESCE(NULLIF(s.title,''),'未命名会话'),COALESCE(s.source,'chat'),COALESCE(NULLIF(trim(s.workspace_root),''),?3)
        FROM sessions s WHERE {VISIBLE} AND (?1='' OR instr(lower(COALESCE(s.title,'')),lower(?1))>0 OR instr(lower(s.id),lower(?1))>0)
        ORDER BY CASE WHEN lower(COALESCE(s.title,''))=lower(?1) THEN 0 ELSE 1 END,s.updated_at DESC,s.id LIMIT ?4"))?;
    let sessions = sessions
        .query_map(
            params![query, include_mobile, default_workspace, per_kind],
            |r| {
                Ok(SearchHit {
                    kind: "session".into(),
                    title: r.get(1)?,
                    session_id: Some(r.get(0)?),
                    artifact_id: None,
                    source: r.get(2)?,
                    workspace_root: r.get(3)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut artifacts = conn.prepare(&format!("SELECT a.id,a.session_id,a.name,COALESCE(s.source,'chat'),COALESCE(NULLIF(trim(s.workspace_root),''),?3)
        FROM session_artifacts a JOIN sessions s ON s.id=a.session_id WHERE {VISIBLE}
        AND (?1='' OR instr(lower(a.name),lower(?1))>0 OR instr(lower(COALESCE(a.uri,'')),lower(?1))>0)
        ORDER BY a.created_at DESC,a.id LIMIT ?4"))?;
    let artifacts = artifacts
        .query_map(
            params![query, include_mobile, default_workspace, per_kind],
            |r| {
                Ok(SearchHit {
                    kind: "artifact".into(),
                    title: r.get(2)?,
                    session_id: Some(r.get(1)?),
                    artifact_id: Some(r.get(0)?),
                    source: r.get(3)?,
                    workspace_root: r.get(4)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut workspaces = conn.prepare(&format!("WITH visible AS (
        SELECT s.id,COALESCE(s.source,'chat') AS source,s.updated_at,COALESCE(NULLIF(trim(s.workspace_root),''),?3) AS root
        FROM sessions s WHERE {VISIBLE}
        ), ranked AS (SELECT *,ROW_NUMBER() OVER (PARTITION BY root ORDER BY updated_at DESC,id) AS position FROM visible)
        SELECT id,source,root FROM ranked WHERE position=1 AND root!='' AND (?1='' OR instr(lower(root),lower(?1))>0)
        ORDER BY updated_at DESC,id LIMIT ?4"))?;
    let mut workspaces = workspaces
        .query_map(
            params![query, include_mobile, default_workspace, per_kind],
            |r| {
                Ok(SearchHit {
                    kind: "workspace".into(),
                    title: r.get(2)?,
                    session_id: Some(r.get(0)?),
                    artifact_id: None,
                    workspace_root: Some(r.get(2)?),
                    source: r.get(1)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !default_workspace.trim().is_empty()
        && (query.is_empty()
            || default_workspace
                .to_lowercase()
                .contains(&query.to_lowercase()))
        && !workspaces
            .iter()
            .any(|h| h.workspace_root.as_deref() == Some(default_workspace))
    {
        workspaces.push(SearchHit {
            kind: "workspace".into(),
            title: default_workspace.into(),
            session_id: None,
            artifact_id: None,
            workspace_root: Some(default_workspace.into()),
            source: "workspace".into(),
        });
    }
    let total = sessions.len() + workspaces.len() + artifacts.len();
    let mut hits = Vec::with_capacity(limit);
    // Interleave real categories rather than letting a large conversation list
    // hide every project/artifact match under the single global result bound.
    let mut groups = [
        sessions.into_iter(),
        workspaces.into_iter(),
        artifacts.into_iter(),
    ];
    while hits.len() < limit {
        let before = hits.len();
        for group in &mut groups {
            if let Some(mut hit) = group.next() {
                hit.title = hit.title.chars().take(512).collect();
                hits.push(hit);
                if hits.len() == limit {
                    break;
                }
            }
        }
        if hits.len() == before {
            break;
        }
    }
    Ok((hits, total > limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE sessions(id TEXT PRIMARY KEY,title TEXT,source TEXT,workspace_root TEXT,updated_at TEXT,archived_at TEXT);
            CREATE TABLE session_artifacts(id TEXT PRIMARY KEY,session_id TEXT,name TEXT,uri TEXT,created_at TEXT);").unwrap();
        conn
    }
    fn session(conn: &Connection, id: &str, title: &str, source: &str, root: &str) {
        conn.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,?4,'2026-10-26',NULL)",
            params![id, title, source, root],
        )
        .unwrap();
    }
    fn artifact(conn: &Connection, id: &str, owner: &str, name: &str) {
        conn.execute(
            "INSERT INTO session_artifacts VALUES (?1,?2,?3,?3,'2026-10-26')",
            params![id, owner, name],
        )
        .unwrap();
    }

    #[test]
    fn searches_all_local_categories_with_correct_artifact_owner_not_just_tasks() {
        let c = fixture();
        session(&c, "s1", "季度方案", "chat", "/work/季度项目");
        session(&c, "s2", "销售报告", "im_feishu", "/work/sales");
        artifact(&c, "a2", "s2", "季度成果.xlsx");
        let (hits, _) = search_hits(&c, "季度", 40, false, "/work/default").unwrap();
        assert!(hits
            .iter()
            .any(|h| h.kind == "session" && h.session_id.as_deref() == Some("s1")));
        assert!(hits.iter().any(
            |h| h.kind == "workspace" && h.workspace_root.as_deref() == Some("/work/季度项目")
        ));
        assert!(hits.iter().any(|h| h.kind == "artifact"
            && h.artifact_id.as_deref() == Some("a2")
            && h.session_id.as_deref() == Some("s2")));
        assert!(!hits
            .iter()
            .any(|h| h.title == "销售报告" && h.kind == "session"));
    }

    #[test]
    fn internal_archived_or_unauthenticated_mobile_sources_do_not_leak_through_artifacts_or_projects(
    ) {
        let c = fixture();
        for (id, source) in [
            ("heartbeat", "chat"),
            ("koi_secret", "chat"),
            ("pool", "piscis_pool"),
            ("h", "codex_heartbeat_global"),
            ("executor", "mobile_executor"),
            ("phone", "mobile"),
            ("unknown-phone", "mobile_other_account"),
        ] {
            session(&c, id, "secret", source, "/secret");
            artifact(&c, &format!("a-{id}"), id, "secret.pdf");
        }
        session(&c, "archived", "secret", "chat", "/secret");
        c.execute(
            "UPDATE sessions SET archived_at='2026' WHERE id='archived'",
            [],
        )
        .unwrap();
        artifact(&c, "archived-a", "archived", "secret.pdf");
        assert!(search_hits(&c, "secret", 40, false, "/default")
            .unwrap()
            .0
            .is_empty());
        let with_auth = search_hits(&c, "secret", 40, true, "/default").unwrap().0;
        assert!(!with_auth.is_empty());
        assert!(with_auth
            .iter()
            .all(|h| h.session_id.as_deref() == Some("phone")));
    }

    #[test]
    fn literal_sql_metacharacters_never_expand_the_search_and_limits_are_enforced() {
        let c = fixture();
        session(&c, "percent", "100%_done", "chat", "/work");
        session(&c, "other", "unrelated", "chat", "/elsewhere");
        let hits = search_hits(&c, "%_", 40, false, "").unwrap().0;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session_id.as_deref(), Some("percent"));
        assert!(search_hits(&c, "' OR 1=1 --", 40, false, "")
            .unwrap()
            .0
            .is_empty());
        assert!(search_hits(&c, &"界".repeat(MAX_QUERY_CHARS + 1), 40, false, "").is_err());
        for n in 0..100 {
            session(&c, &format!("row-{n}"), "many", "chat", "/many");
        }
        let (hits, more) = search_hits(&c, "many", usize::MAX, false, "").unwrap();
        assert!(hits.len() <= MAX_RESULTS);
        assert!(more);
    }

    #[test]
    fn empty_query_is_bounded_recent_content_and_default_workspace_is_real_metadata() {
        let c = fixture();
        session(&c, "s", "Planning", "chat", "");
        let (hits, _) = search_hits(&c, "", 10, false, "/work/default").unwrap();
        assert!(
            hits.iter()
                .any(|h| h.kind == "workspace"
                    && h.workspace_root.as_deref() == Some("/work/default"))
        );
        assert!(hits
            .iter()
            .any(|h| h.kind == "session" && h.title == "Planning"));
    }
}
