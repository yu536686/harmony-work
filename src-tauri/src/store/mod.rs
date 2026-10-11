pub mod db;
pub mod secrets;
pub mod settings;
mod vector;

pub use db::Database;
pub use settings::{Settings, SkillEvolutionSettings};

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::lsp::manager::LspManager;

fn database_path(app_dir: &Path) -> PathBuf {
    app_dir.join("workbuddy.db")
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
        std::fs::create_dir_all(&app_dir)?;

        let db_path = database_path(&app_dir);
        let db = Database::open(&db_path)?;

        let config_path = app_dir.join("config.json");
        let mut settings = Settings::load(&config_path)?;
        if crate::commands::config::bundled_mcp::strip_legacy_robotz_mcp_server(&mut settings) {
            settings.save().map_err(|e| anyhow::anyhow!("{e}"))?;
        }

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
    use super::{database_path, Database};

    #[test]
    fn database_path_stays_within_current_app_data() {
        let root = std::env::temp_dir().join(format!("workbuddy-isolation-{}", uuid::Uuid::new_v4()));
        let old_product = root.join("old-product");
        let current = root.join("current");
        std::fs::create_dir_all(&old_product).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(old_product.join("legacy.db"), "old data").unwrap();

        let path = database_path(&current);
        Database::open(&path).unwrap();
        assert!(path.exists());
        assert!(!current.join("legacy.db").exists());
        assert_eq!(std::fs::read_to_string(old_product.join("legacy.db")).unwrap(), "old data");
        let _ = std::fs::remove_dir_all(root);
    }
}
