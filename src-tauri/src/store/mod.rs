pub mod db;
pub mod secrets;
pub mod settings;
mod vector;

pub use db::Database;
pub use settings::{Settings, SkillEvolutionSettings};

use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::lsp::manager::LspManager;

fn copy_missing_tree(source: &Path, target: &Path) -> Result<()> {
    if !source.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination = target.join(entry.file_name());
        if file_type.is_dir() {
            copy_missing_tree(&entry.path(), &destination)?;
        } else if file_type.is_file() && !destination.exists() {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn migrate_legacy_app_data_from(legacy_dir: &Path, app_dir: &Path) -> Result<()> {
    copy_missing_tree(legacy_dir, app_dir)
}

fn migrate_legacy_app_data(app_dir: &Path) -> Result<()> {
    let Some(data_root) = app_dir.parent() else {
        return Ok(());
    };
    let legacy_dir = data_root.join("com.piscis.desktop");
    if legacy_dir == app_dir || !legacy_dir.is_dir() {
        return Ok(());
    }
    migrate_legacy_app_data_from(&legacy_dir, app_dir)
}

fn database_has_table(path: &Path, table: &str) -> bool {
    if !path.is_file() {
        return false;
    }
    Connection::open(path)
        .and_then(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                params![table],
                |row| row.get::<_, i64>(0),
            )
        })
        .map(|exists| exists != 0)
        .unwrap_or(false)
}

fn migrate_database_file(app_dir: &Path) -> Result<PathBuf> {
    let target = app_dir.join("workbuddy.db");
    let previous = app_dir.join("piscis.db");
    if !previous.is_file() || database_has_table(&target, "sessions") {
        return Ok(target);
    }

    let memory_backup = app_dir.join("workbuddy.memory-backup.db");
    if target.is_file() {
        let _ = std::fs::remove_file(&memory_backup);
        std::fs::rename(&target, &memory_backup)?;
    }
    std::fs::rename(&previous, &target)?;
    for suffix in ["-wal", "-shm"] {
        let source = app_dir.join(format!("piscis.db{suffix}"));
        let destination = app_dir.join(format!("workbuddy.db{suffix}"));
        if source.is_file() {
            let _ = std::fs::rename(source, destination);
        }
    }

    if memory_backup.is_file() {
        let connection = Connection::open(&target)?;
        connection.execute(
            "ATTACH DATABASE ?1 AS memory_backup",
            params![memory_backup.display().to_string()],
        )?;
        let has_backup_memories: i64 = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_backup.sqlite_master WHERE type='table' AND name='memories')",
            [],
            |row| row.get(0),
        )?;
        if has_backup_memories != 0 {
            let _ = connection.execute(
                "INSERT OR IGNORE INTO memories (id, content, category, confidence, owner_id, scope_type, scope_id, kind, last_seen_at, created_at, updated_at)
                 SELECT id, content, category, 0.8,
                        CASE WHEN owner_id='piscis' THEN 'xiaoheng' ELSE owner_id END,
                        'private',
                        CASE WHEN owner_id='piscis' THEN 'xiaoheng' ELSE owner_id END,
                        'fact', created_at, created_at, created_at
                 FROM memory_backup.memories",
                [],
            );
        }
        let _ = connection.execute_batch("DETACH DATABASE memory_backup");
        let _ = std::fs::remove_file(memory_backup);
    }
    Ok(target)
}

/// Global application state managed by Tauri
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub settings: Arc<Mutex<Settings>>,
    /// Current visible execution plan per session
    pub plan_state:
        Arc<Mutex<std::collections::HashMap<String, Vec<crate::agent_core::plan::PlanTodoItem>>>>,
    /// Active agent cancellation tokens: session_id -> cancel flag
    pub cancel_flags:
        Arc<Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
    /// Shared browser manager (Chrome for Testing)
    pub browser: robotz_browser::SharedBrowserManager,
    /// Cron scheduler for recurring tasks
    pub scheduler: Arc<crate::scheduler::CronScheduler>,
    /// Active cron job ids keyed by scheduled task id so updates/restarts can replace jobs instead of duplicating them.
    pub scheduled_job_ids: Arc<Mutex<std::collections::HashMap<String, uuid::Uuid>>>,
    /// App handle for emitting events from scheduler tasks
    pub app_handle: AppHandle,
    /// Pending permission confirmation channels: request_id -> oneshot sender
    pub confirmation_responses:
        Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    /// Pending interactive UI response channels: request_id -> oneshot sender
    pub interactive_responses: Arc<
        Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<serde_json::Value>>>,
    >,
    /// IM gateway manager
    pub gateway: Arc<crate::gateway::GatewayManager>,
    /// Per-pool heartbeat cursor: pool_id -> last processed pool_message.id
    pub coordination_heartbeat_cursor: Arc<Mutex<std::collections::HashMap<String, i64>>>,
    /// IDE terminal session registry
    pub terminals: Arc<Mutex<crate::commands::ide::TerminalRegistry>>,
    /// IDE file watcher registry: project_dir -> notify watcher handle
    pub file_watchers: Arc<Mutex<std::collections::HashMap<String, notify::RecommendedWatcher>>>,
    /// LSP (Language Server Protocol) session manager
    pub lsp_manager: Arc<LspManager>,
    /// Live shell/file-write confirmation prefs — agent loops read on each tool call.
    pub confirm_flags: crate::agent_core::approval::ConfirmFlagsHandle,
}

impl AppState {
    /// Synchronous construction — scheduler must be provided after async init.
    pub fn new_sync(app: &AppHandle, scheduler: crate::scheduler::CronScheduler) -> Result<Self> {
        let app_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
        Self::new_sync_with_app_dir(app, scheduler, app_dir)
    }

    /// Synchronous construction with an explicit app-data directory.
    /// Used by headless/CLI entry points so they can run against an isolated
    /// config + database root without mutating the desktop app's default state.
    pub fn new_sync_with_app_dir(
        app: &AppHandle,
        scheduler: crate::scheduler::CronScheduler,
        app_dir: std::path::PathBuf,
    ) -> Result<Self> {
        migrate_legacy_app_data(&app_dir)?;
        std::fs::create_dir_all(&app_dir)?;

        let db_path = migrate_database_file(&app_dir)?;
        let db = Database::open(&db_path)?;

        let config_path = app_dir.join("config.json");
        let mut settings = Settings::load(&config_path)?;
        if crate::commands::config::bundled_mcp::strip_legacy_robotz_mcp_server(&mut settings) {
            settings.save().map_err(|e| anyhow::anyhow!("{e}"))?;
        }

        let browser_options = robotz_browser::BrowserOptions {
            chrome_dir: app_dir.join("chrome"),
            headless: settings.browser_headless,
            ..Default::default()
        };

        let confirm_flags = crate::agent_core::approval::confirm_flags_handle(
            settings.confirm_shell_commands,
            settings.confirm_file_writes,
        );

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            settings: Arc::new(Mutex::new(settings)),
            confirm_flags,
            plan_state: Arc::new(Mutex::new(std::collections::HashMap::new())),
            cancel_flags: Arc::new(Mutex::new(std::collections::HashMap::new())),
            browser: robotz_browser::create_browser_manager(browser_options),
            scheduler: Arc::new(scheduler),
            scheduled_job_ids: Arc::new(Mutex::new(std::collections::HashMap::new())),
            app_handle: app.clone(),
            confirmation_responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
            interactive_responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
            gateway: Arc::new(crate::gateway::GatewayManager::new()),
            coordination_heartbeat_cursor: Arc::new(Mutex::new(std::collections::HashMap::new())),
            terminals: Arc::new(Mutex::new(crate::commands::ide::TerminalRegistry::new())),
            file_watchers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            lsp_manager: Arc::new(LspManager::with_bridge_name("Harmony Work")),
        })
    }
}

#[cfg(test)]
mod migration_tests {
    use super::migrate_legacy_app_data_from;

    #[test]
    fn legacy_app_data_is_copied_without_overwriting_current_files() {
        let root =
            std::env::temp_dir().join(format!("workbuddy-migration-{}", uuid::Uuid::new_v4()));
        let legacy = root.join("legacy");
        let current = root.join("current");
        std::fs::create_dir_all(legacy.join("skills")).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(legacy.join("config.json"), "legacy").unwrap();
        std::fs::write(legacy.join("skills").join("sample.md"), "skill").unwrap();
        std::fs::write(current.join("config.json"), "current").unwrap();

        migrate_legacy_app_data_from(&legacy, &current).unwrap();

        assert_eq!(
            std::fs::read_to_string(current.join("config.json")).unwrap(),
            "current"
        );
        assert_eq!(
            std::fs::read_to_string(current.join("skills").join("sample.md")).unwrap(),
            "skill"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
