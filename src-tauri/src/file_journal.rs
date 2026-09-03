//! Compatibility reader for file snapshots recorded by earlier WorkBuddy builds.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// One journaled file change, surfaced to a host's review UI.
#[derive(Debug, Clone, Serialize)]
pub struct JournalChange {
    pub id: i64,
    pub rel_path: String,
    pub tool_name: String,
    /// Whether the file existed before the edit (false => agent created it).
    pub existed: bool,
    /// Whether the edit actually applied (tool returned success).
    pub applied: bool,
}

/// Workspace-scoped file journal. Cheap to open; backed by a SQLite file.
pub struct FileJournal {
    conn: Mutex<Connection>,
    workspace_root: PathBuf,
}

impl FileJournal {
    /// Open (and migrate) the journal DB. `workspace_root` is the directory
    /// tool paths are resolved against; `db_path` is where snapshots live
    /// (its parent directory is created if missing).
    pub fn open(workspace_root: impl AsRef<Path>, db_path: impl AsRef<Path>) -> Result<Self> {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create journal dir {}", parent.display()))?;
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("open journal db {}", db_path.display()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS file_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                tool_use_id TEXT,
                tool_name TEXT NOT NULL,
                rel_path TEXT NOT NULL,
                existed INTEGER NOT NULL,
                before_content BLOB,
                applied INTEGER NOT NULL DEFAULT 0,
                undone INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_snap_session_turn
                ON file_snapshots(session_id, turn_id);",
        )
        .context("journal migrate failed")?;
        Ok(Self {
            conn: Mutex::new(conn),
            workspace_root,
        })
    }

    fn abs_path(&self, rel: &str) -> PathBuf {
        self.workspace_root.join(rel)
    }

    /// The most recent turn id that still has applied, not-yet-undone changes
    /// for a session. Lets event-driven hosts (whose turn handler returns before
    /// the agent finishes) resolve "the last turn" without threading a turn id.
    pub fn latest_turn_with_changes(&self, session_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let id = conn
            .query_row(
                "SELECT turn_id FROM file_snapshots
                 WHERE session_id = ?1 AND applied = 1 AND undone = 0
                 ORDER BY id DESC LIMIT 1",
                params![session_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(id)
    }

    /// List applied, not-yet-undone changes for a turn (most recent first,
    /// deduped by path so each file appears once).
    pub fn list_changes(&self, session_id: &str, turn_id: &str) -> Result<Vec<JournalChange>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, rel_path, tool_name, existed, applied
             FROM file_snapshots
             WHERE session_id = ?1 AND turn_id = ?2 AND applied = 1 AND undone = 0
             ORDER BY id DESC",
        )?;
        let rows = stmt.query_map(params![session_id, turn_id], |r| {
            Ok(JournalChange {
                id: r.get(0)?,
                rel_path: r.get(1)?,
                tool_name: r.get(2)?,
                existed: r.get::<_, i64>(3)? != 0,
                applied: r.get::<_, i64>(4)? != 0,
            })
        })?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for row in rows {
            let c = row?;
            if seen.insert(c.rel_path.clone()) {
                out.push(c);
            }
        }
        Ok(out)
    }

    /// Undo all applied changes in a turn, restoring pre-edit content.
    /// Returns the list of restored relative paths.
    pub fn undo_turn(&self, session_id: &str, turn_id: &str) -> Result<Vec<String>> {
        // Reverse application order so later edits to the same file are unwound
        // before earlier ones, leaving the oldest pre-edit content as final.
        let snapshots: Vec<(i64, String, bool, Option<Vec<u8>>)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, rel_path, existed, before_content
                 FROM file_snapshots
                 WHERE session_id = ?1 AND turn_id = ?2 AND applied = 1 AND undone = 0
                 ORDER BY id DESC",
            )?;
            let rows = stmt.query_map(params![session_id, turn_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? != 0,
                    r.get::<_, Option<Vec<u8>>>(3)?,
                ))
            })?;
            let mut v = Vec::new();
            for row in rows {
                v.push(row?);
            }
            v
        };

        let mut restored = Vec::new();
        let mut restored_paths = std::collections::HashSet::new();
        for (_id, rel, existed, before) in &snapshots {
            // Only act once per path (the latest snapshot wins for restore).
            if !restored_paths.insert(rel.clone()) {
                continue;
            }
            let abs = self.abs_path(rel);
            if *existed {
                if let Some(bytes) = before {
                    if let Some(parent) = abs.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(&abs, bytes).with_context(|| format!("restore {rel} failed"))?;
                }
            } else {
                // File was created by the agent — remove it to undo.
                let _ = std::fs::remove_file(&abs);
            }
            restored.push(rel.clone());
        }

        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE file_snapshots SET undone = 1
             WHERE session_id = ?1 AND turn_id = ?2 AND applied = 1",
            params![session_id, turn_id],
        );
        Ok(restored)
    }
}
