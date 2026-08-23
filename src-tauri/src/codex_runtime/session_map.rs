use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexSessionBinding {
    pub workbuddy_session_id: String,
    pub codex_thread_id: String,
    pub workspace_root: String,
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy)]
pub struct NewCodexSessionBinding<'a> {
    pub workbuddy_session_id: &'a str,
    pub codex_thread_id: &'a str,
    pub workspace_root: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
}

#[derive(Debug, thiserror::Error)]
pub enum CodexSessionMapError {
    #[error("Codex session map database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Codex session map lock was poisoned")]
    LockPoisoned,
}

pub struct CodexSessionMapStore {
    connection: Mutex<Connection>,
}

impl CodexSessionMapStore {
    pub fn open(path: &Path) -> Result<Self, CodexSessionMapError> {
        let connection = Connection::open(path)?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, CodexSessionMapError> {
        let store = Self {
            connection: Mutex::new(Connection::open_in_memory()?),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn bind(&self, binding: NewCodexSessionBinding<'_>) -> Result<(), CodexSessionMapError> {
        let now = Utc::now().to_rfc3339();
        self.connection()?.execute(
            "INSERT INTO codex_session_bindings (
                workbuddy_session_id, codex_thread_id, workspace_root, provider, model,
                created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(workbuddy_session_id) DO UPDATE SET
                codex_thread_id = excluded.codex_thread_id,
                workspace_root = excluded.workspace_root,
                provider = excluded.provider,
                model = excluded.model,
                updated_at = excluded.updated_at",
            params![
                binding.workbuddy_session_id,
                binding.codex_thread_id,
                binding.workspace_root,
                binding.provider,
                binding.model,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get(
        &self,
        workbuddy_session_id: &str,
    ) -> Result<Option<CodexSessionBinding>, CodexSessionMapError> {
        self.connection()?
            .query_row(
                "SELECT workbuddy_session_id, codex_thread_id, workspace_root, provider, model,
                        created_at, updated_at
                 FROM codex_session_bindings
                 WHERE workbuddy_session_id = ?1",
                params![workbuddy_session_id],
                |row| {
                    Ok(CodexSessionBinding {
                        workbuddy_session_id: row.get(0)?,
                        codex_thread_id: row.get(1)?,
                        workspace_root: row.get(2)?,
                        provider: row.get(3)?,
                        model: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(CodexSessionMapError::from)
    }

    pub fn remove(&self, workbuddy_session_id: &str) -> Result<bool, CodexSessionMapError> {
        let changed = self.connection()?.execute(
            "DELETE FROM codex_session_bindings WHERE workbuddy_session_id = ?1",
            params![workbuddy_session_id],
        )?;
        Ok(changed > 0)
    }

    pub fn count(&self) -> Result<u64, CodexSessionMapError> {
        self.connection()?
            .query_row("SELECT COUNT(*) FROM codex_session_bindings", [], |row| {
                row.get(0)
            })
            .map_err(CodexSessionMapError::from)
    }

    fn migrate(&self) -> Result<(), CodexSessionMapError> {
        self.connection()?.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS codex_session_bindings (
                workbuddy_session_id TEXT PRIMARY KEY,
                codex_thread_id TEXT NOT NULL,
                workspace_root TEXT NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE UNIQUE INDEX IF NOT EXISTS idx_codex_session_thread
                ON codex_session_bindings(codex_thread_id);",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, CodexSessionMapError> {
        self.connection
            .lock()
            .map_err(|_| CodexSessionMapError::LockPoisoned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_workbuddy_to_codex_thread_mapping() {
        let store = CodexSessionMapStore::open_in_memory().expect("store");
        store
            .bind(NewCodexSessionBinding {
                workbuddy_session_id: "session-1",
                codex_thread_id: "thread-1",
                workspace_root: "/work/project-a",
                provider: "deepseek",
                model: "deepseek-v4-pro",
            })
            .expect("bind");

        let binding = store.get("session-1").expect("get").expect("binding");
        assert_eq!(binding.codex_thread_id, "thread-1");
        assert_eq!(binding.workspace_root, "/work/project-a");
        assert_eq!(binding.provider, "deepseek");
        assert_eq!(binding.model, "deepseek-v4-pro");
    }

    #[test]
    fn rebinding_updates_thread_without_duplicate_rows() {
        let store = CodexSessionMapStore::open_in_memory().expect("store");
        for thread in ["thread-old", "thread-new"] {
            store
                .bind(NewCodexSessionBinding {
                    workbuddy_session_id: "session-1",
                    codex_thread_id: thread,
                    workspace_root: "/work",
                    provider: "openai",
                    model: "gpt-5.3-codex",
                })
                .expect("bind");
        }

        assert_eq!(store.count().expect("count"), 1);
        assert_eq!(
            store
                .get("session-1")
                .expect("get")
                .expect("binding")
                .codex_thread_id,
            "thread-new"
        );
    }

    #[test]
    fn removing_workbuddy_session_removes_mapping() {
        let store = CodexSessionMapStore::open_in_memory().expect("store");
        store
            .bind(NewCodexSessionBinding {
                workbuddy_session_id: "session-1",
                codex_thread_id: "thread-1",
                workspace_root: "/work",
                provider: "openai",
                model: "gpt-5.3-codex",
            })
            .expect("bind");

        assert!(store.remove("session-1").expect("remove"));
        assert!(store.get("session-1").expect("get").is_none());
    }
}
