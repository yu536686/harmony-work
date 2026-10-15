use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

fn normalize_koi_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("Koi 名称不能为空"));
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err(anyhow::anyhow!("Koi 名称不能包含空格或其他空白字符"));
    }
    if trimmed.chars().any(is_disallowed_koi_name_char) {
        return Err(anyhow::anyhow!(
            "Koi 名称不能包含 emoji 或其他 pictographic 字符"
        ));
    }
    Ok(trimmed.to_string())
}

fn is_disallowed_koi_name_char(ch: char) -> bool {
    let cp = ch as u32;
    matches!(
        cp,
        0x200D
            | 0xFE0F
            | 0x1F1E6..=0x1F1FF
            | 0x1F300..=0x1FAFF
            | 0x2600..=0x27BF
            | 0x2300..=0x23FF
    )
}

// ---------------------------------------------------------------------------
// Data models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    /// Origin of this session: "chat" (UI), "im_telegram", "im_feishu", etc.
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: i64,
    #[serde(default)]
    pub rolling_summary: String,
    #[serde(default)]
    pub rolling_summary_version: i64,
    #[serde(default)]
    pub total_input_tokens: i64,
    #[serde(default)]
    pub total_output_tokens: i64,
    #[serde(default)]
    pub last_compacted_at: Option<DateTime<Utc>>,
    /// Per-session workspace override. When set, this replaces the global
    /// `workspace_root` from settings for all tool operations and prompt
    /// injection in this session. NULL = use global workspace_root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    /// Explicit binding to a pool (team) session for team-task main chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_session_id: Option<String>,
    /// Per-session tool policy override: "strict" | "balanced" | "dev". NULL = inherit global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_mode: Option<String>,
    /// Per-session override for accessing files outside workspace. NULL = inherit global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_outside_workspace: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContextState {
    pub session_id: String,
    pub rolling_summary: String,
    pub rolling_summary_version: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub last_compacted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImSessionBinding {
    pub binding_key: String,
    pub channel: String,
    pub external_conversation_key: String,
    pub session_id: String,
    pub peer_id: String,
    pub peer_name: Option<String>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub latest_reply_target: String,
    pub routing_state_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_inbound_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ImSessionBindingUpsert {
    pub binding_key: String,
    pub channel: String,
    pub external_conversation_key: String,
    pub session_id: String,
    pub peer_id: String,
    pub peer_name: Option<String>,
    pub is_group: bool,
    pub group_name: Option<String>,
    pub latest_reply_target: String,
    pub routing_state_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    /// JSON array of ToolUse blocks for assistant messages that made tool calls.
    /// Serialized form of Vec<ContentBlock::ToolUse>.
    #[serde(default)]
    pub tool_calls_json: Option<String>,
    /// JSON array of ToolResult blocks for user messages that carry tool results.
    /// Serialized form of Vec<ContentBlock::ToolResult>.
    #[serde(default)]
    pub tool_results_json: Option<String>,
    /// 1-based index of the conversation turn this message belongs to.
    /// A "turn" starts with each user message.
    #[serde(default)]
    pub turn_index: Option<i64>,
    /// Structured, non-visible context attached to the user message, such as
    /// artifact annotations selected for a follow-up turn.
    #[serde(default)]
    pub context_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionArtifact {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub artifact_type: String,
    pub uri: Option<String>,
    pub content_summary: String,
    pub source_tool: Option<String>,
    pub tool_use_id: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnExecutionEventRecord {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub category: String,
    pub confidence: f64,
    pub source_session_id: Option<String>,
    pub memory_type: String,
    /// Who owns this memory: "xiaoheng" or a staff id.
    pub owner_id: String,
    /// "private" | "project" | "global"
    pub scope_type: String,
    /// For project scope: pool_session_id; for private: same as owner_id; for global: "global"
    pub scope_id: String,
    /// For private memories: the pool_session_id where this memory was created (NULL = cross-project skill/preference)
    pub project_scope_id: Option<String>,
    /// Phase 4d: shard kind in the structured rolling summary sense.
    /// One of "fact" | "decision" | "preference" | "error_learned" | "open_item".
    /// Defaults to "fact" for legacy rows and untyped saves.
    #[serde(default = "default_memory_kind")]
    pub kind: String,
    /// Phase 4d: FEC anchor — session this memory was derived from.
    #[serde(default)]
    pub evidence_session_id: Option<String>,
    /// Phase 4d: FEC anchor — concrete tool exchange the memory
    /// references, enabling `recall_tool_result` retrieval.
    #[serde(default)]
    pub evidence_tool_use_id: Option<String>,
    /// Phase 4d: most recent re-observation time. Distinct from
    /// `updated_at` (which reflects content edits) so confidence-bump
    /// re-observations don't clobber the edit timestamp used by UI
    /// sorting.
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCloudChange {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub memory: Option<Memory>,
}

fn default_memory_kind() -> String {
    "fact".to_string()
}

/// Phase 4d: optional structured fields for [`Database::save_memory_structured`].
/// All defaults preserve legacy `save_memory` behaviour.
#[derive(Debug, Clone, Default)]
pub struct MemorySaveExtras {
    pub kind: Option<String>,
    pub evidence_session_id: Option<String>,
    pub evidence_tool_use_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cron_expression: String,
    pub task_prompt: String,
    pub notify_targets_json: Option<String>,
    pub status: String,
    pub last_run_status: Option<String>,
    pub run_count: i64,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub icon: String,
    pub config: String, // JSON string
}

/// One recorded change to a skill (patch/edit/create).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRevision {
    pub id: String,
    pub skill_id: String,
    pub session_id: Option<String>,
    pub origin: String,
    pub diff_summary: Option<String>,
    pub content_before_hash: Option<String>,
    pub content_after_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Per-skill usage telemetry for curator and evolution policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUsage {
    pub skill_id: String,
    pub view_count: i64,
    pub use_count: i64,
    pub patch_count: i64,
    pub last_used_at: Option<DateTime<Utc>>,
    pub last_patched_at: Option<DateTime<Utc>>,
    pub state: String,
    pub pinned: bool,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSnapshot {
    pub id: String,
    pub session_id: String,
    /// Short label for the turn (user goal snippet or auto-generated).
    pub label: String,
    /// JSON array of plan_todo items from the chat UI.
    pub items_json: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    pub action: String,
    pub input_summary: Option<String>,
    pub result_summary: Option<String>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub scope_type: String,
    pub scope_id: String,
    pub goal: String,
    pub state_json: String,
    pub summary: String,
    pub status: String,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskSpine {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub current_step: String,
    #[serde(default)]
    pub done: Vec<String>,
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub next_questions: Vec<String>,
}

impl TaskState {
    pub fn to_task_spine(&self) -> TaskSpine {
        let mut spine = serde_json::from_str::<TaskSpine>(&self.state_json).unwrap_or_default();
        if spine.goal.trim().is_empty() {
            spine.goal = self.goal.clone();
        }
        if spine.current_step.trim().is_empty() && !self.summary.trim().is_empty() {
            spine.current_step = self.summary.clone();
        }
        spine
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

pub struct Database {
    /// Low-level SQLite handle. Kept `pub` so host crates (desktop, cli) can
    /// run ad-hoc migrations and test harness queries without the kernel
    /// needing to expose a bespoke accessor for each one.
    pub conn: Connection,
}

fn parse_datetime(value: String) -> DateTime<Utc> {
    value
        .parse::<DateTime<Utc>>()
        .unwrap_or_else(|_| Utc::now())
}

fn parse_optional_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| raw.parse::<DateTime<Utc>>().ok())
}

fn parse_optional_bool(value: Option<i64>) -> Option<bool> {
    value.map(|v| v != 0)
}

fn map_session_row(r: &Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        title: r.get(1)?,
        status: r.get(2)?,
        source: r.get(3)?,
        created_at: parse_datetime(r.get::<_, String>(4)?),
        updated_at: parse_datetime(r.get::<_, String>(5)?),
        message_count: r.get(6)?,
        rolling_summary: r.get(7)?,
        rolling_summary_version: r.get(8)?,
        total_input_tokens: r.get(9)?,
        total_output_tokens: r.get(10)?,
        last_compacted_at: parse_optional_datetime(r.get::<_, Option<String>>(11)?),
        workspace_root: r.get(12).ok().flatten(),
        pinned_at: parse_optional_datetime(r.get::<_, Option<String>>(13).ok().flatten()),
        archived_at: parse_optional_datetime(r.get::<_, Option<String>>(14).ok().flatten()),
        pool_session_id: r.get(15).ok().flatten(),
        policy_mode: r.get(16).ok().flatten(),
        allow_outside_workspace: parse_optional_bool(r.get::<_, Option<i64>>(17).ok().flatten()),
    })
}

fn map_chat_message_row(r: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    Ok(ChatMessage {
        id: r.get(0)?,
        session_id: r.get(1)?,
        role: r.get(2)?,
        content: r.get(3)?,
        created_at: parse_datetime(r.get::<_, String>(4)?),
        tool_calls_json: r.get(5)?,
        tool_results_json: r.get(6)?,
        turn_index: r.get(7)?,
        context_json: r.get(8)?,
    })
}

fn map_im_session_binding_row(r: &Row<'_>) -> rusqlite::Result<ImSessionBinding> {
    Ok(ImSessionBinding {
        binding_key: r.get(0)?,
        channel: r.get(1)?,
        external_conversation_key: r.get(2)?,
        session_id: r.get(3)?,
        peer_id: r.get(4)?,
        peer_name: r.get(5)?,
        is_group: r.get(6)?,
        group_name: r.get(7)?,
        latest_reply_target: r.get(8)?,
        routing_state_json: r.get(9)?,
        created_at: parse_datetime(r.get::<_, String>(10)?),
        updated_at: parse_datetime(r.get::<_, String>(11)?),
        last_inbound_at: parse_datetime(r.get::<_, String>(12)?),
    })
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {:?}", path))?;

        // Enable WAL mode for better concurrent read performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open an in-memory database for testing.
    /// Uses the same schema migration as production.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("Failed to open in-memory database")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT,
                status TEXT NOT NULL DEFAULT 'idle',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                rolling_summary TEXT NOT NULL DEFAULT '',
                rolling_summary_version INTEGER NOT NULL DEFAULT 0,
                total_input_tokens INTEGER NOT NULL DEFAULT 0,
                total_output_tokens INTEGER NOT NULL DEFAULT 0,
                last_compacted_at TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                context_json TEXT,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, created_at);

            CREATE TABLE IF NOT EXISTS turn_execution_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(turn_id, sequence),
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_turn_execution_session
                ON turn_execution_events(session_id, created_at, sequence);

            CREATE TABLE IF NOT EXISTS im_session_bindings (
                binding_key TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                external_conversation_key TEXT NOT NULL,
                session_id TEXT NOT NULL,
                peer_id TEXT NOT NULL DEFAULT '',
                peer_name TEXT,
                is_group INTEGER NOT NULL DEFAULT 0,
                group_name TEXT,
                latest_reply_target TEXT NOT NULL DEFAULT '',
                routing_state_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_inbound_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_im_session_bindings_session
                ON im_session_bindings(session_id, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_im_session_bindings_channel_key
                ON im_session_bindings(channel, external_conversation_key);

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'general',
                confidence REAL NOT NULL DEFAULT 0.7,
                source_session_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memory_tombstones (
                id TEXT PRIMARY KEY,
                deleted_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS memory_sync_identity (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                user_id TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                cron_expression TEXT NOT NULL,
                task_prompt TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                run_count INTEGER NOT NULL DEFAULT 0,
                last_run_at TEXT,
                next_run_at TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS skills (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                icon TEXT NOT NULL DEFAULT '',
                config TEXT NOT NULL DEFAULT '{}'
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                action TEXT NOT NULL,
                input_summary TEXT,
                result_summary TEXT,
                is_error INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_audit_session ON audit_log(session_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_audit_tool ON audit_log(tool_name, timestamp);

            CREATE TABLE IF NOT EXISTS plan_snapshots (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                items_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_plan_snapshots_session ON plan_snapshots(session_id, created_at);

            CREATE TABLE IF NOT EXISTS skill_revisions (
                id TEXT PRIMARY KEY,
                skill_id TEXT NOT NULL,
                session_id TEXT,
                origin TEXT NOT NULL DEFAULT 'agent',
                diff_summary TEXT,
                content_before_hash TEXT,
                content_after_hash TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_skill_revisions_skill ON skill_revisions(skill_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_skill_revisions_session ON skill_revisions(session_id, created_at);

            CREATE TABLE IF NOT EXISTS skill_usage (
                skill_id TEXT PRIMARY KEY,
                view_count INTEGER NOT NULL DEFAULT 0,
                use_count INTEGER NOT NULL DEFAULT 0,
                patch_count INTEGER NOT NULL DEFAULT 0,
                last_used_at TEXT,
                last_patched_at TEXT,
                state TEXT NOT NULL DEFAULT 'active',
                pinned INTEGER NOT NULL DEFAULT 0,
                created_by TEXT
            );

            CREATE TABLE IF NOT EXISTS session_artifacts (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                name TEXT NOT NULL,
                artifact_type TEXT NOT NULL DEFAULT 'file',
                uri TEXT,
                content_summary TEXT NOT NULL DEFAULT '',
                source_tool TEXT,
                tool_use_id TEXT,
                metadata_json TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_session_artifacts_session
                ON session_artifacts(session_id, created_at DESC);
        ",
        )?;

        // Add last_run_status to scheduled_tasks (ignore if already exists)
        let _ = self.conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN last_run_status TEXT",
            [],
        );

        // Add source column to sessions for IM origin tracking (ignore if already exists)
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN source TEXT NOT NULL DEFAULT 'chat'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN rolling_summary TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN rolling_summary_version INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN total_input_tokens INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN total_output_tokens INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN last_compacted_at TEXT", []);

        // p6 state frame: structured snapshot of "where we are now" that
        // survives across sessions. Kept as raw JSON so we can iterate on
        // the shape without another migration.
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN state_frame_json TEXT", []);

        // Per-session workspace override: when set, replaces global workspace_root
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN workspace_root TEXT", []);

        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN pinned_at TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN archived_at TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN pool_session_id TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE sessions ADD COLUMN policy_mode TEXT", []);
        let _ = self.conn.execute(
            "ALTER TABLE sessions ADD COLUMN allow_outside_workspace INTEGER",
            [],
        );

        // Memory enhancement: add embedding and memory_type columns (ignore if already exist)
        let _ = self
            .conn
            .execute("ALTER TABLE memories ADD COLUMN embedding BLOB", []);
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN memory_type TEXT NOT NULL DEFAULT 'personal'",
            [],
        );

        // Context management: add tool call persistence columns to messages (ignore if already exist)
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN tool_calls_json TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN tool_results_json TEXT", []);
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN turn_index INTEGER", []);
        let _ = self
            .conn
            .execute("ALTER TABLE messages ADD COLUMN context_json TEXT", []);
        let _ = self.conn.execute("ALTER TABLE messages ADD COLUMN server_seq INTEGER", []);
        let _ = self.conn.execute("ALTER TABLE messages ADD COLUMN origin_device_id TEXT", []);
        self.conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_messages_shared_seq ON messages(session_id, server_seq);")?;
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS mobile_desktop_turns (
            turn_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, initiating_message_id TEXT NOT NULL,
            completed INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );")?;

        // Agent checkpoints for crash recovery
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS agent_checkpoints (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                iteration INTEGER NOT NULL,
                messages_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'running',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_checkpoint_session ON agent_checkpoints(session_id, updated_at);
        ")?;

        self.conn.execute_batch("
            -- FTS5 full-text search for memories
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content,
                content=memories,
                content_rowid=rowid
            );

            -- Triggers to keep FTS5 in sync
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content) VALUES('delete', old.rowid, old.content);
                INSERT INTO memories_fts(rowid, content) VALUES (new.rowid, new.content);
            END;

            -- Embedding cache to avoid redundant API calls
            CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                created_at TEXT NOT NULL
            );
        ")?;

        // Fish instances table (user-activated sub-Agents)
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS fish_instances (
                fish_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                user_config TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        ",
        );

        // Task state table for structured task progress tracking.
        // scope_type: 'session' (chat) or 'scheduled_task' (scheduler).
        // state_json stores structured progress: goal, done_items, pending_items, etc.
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS task_states (
                id TEXT PRIMARY KEY,
                scope_type TEXT NOT NULL DEFAULT 'session',
                scope_id TEXT NOT NULL,
                goal TEXT NOT NULL DEFAULT '',
                state_json TEXT NOT NULL DEFAULT '{}',
                summary TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'active',
                version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_task_states_scope ON task_states(scope_type, scope_id);
        ",
        );

        // ---------- Koi system tables (v2) ----------

        // Koi: persistent independent Agents
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS kois (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT '',
                icon TEXT NOT NULL DEFAULT '🐡',
                color TEXT NOT NULL DEFAULT '#7c6af7',
                system_prompt TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'idle',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                llm_provider_id TEXT,
                max_iterations INTEGER NOT NULL DEFAULT 0,
                task_timeout_secs INTEGER NOT NULL DEFAULT 0
            );
        ",
        );
        // Migrations: add columns to existing kois tables
        let _ = self
            .conn
            .execute("ALTER TABLE kois ADD COLUMN llm_provider_id TEXT", []);
        let _ = self.conn.execute(
            "ALTER TABLE kois ADD COLUMN max_iterations INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE kois ADD COLUMN task_timeout_secs INTEGER NOT NULL DEFAULT 0",
            [],
        );

        // Koi todo items (shared board)
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS koi_todos (
                id TEXT PRIMARY KEY,
                owner_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT DEFAULT '',
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT DEFAULT 'medium',
                assigned_by TEXT DEFAULT '',
                pool_session_id TEXT,
                claimed_by TEXT,
                claimed_at TEXT,
                depends_on TEXT,
                blocked_reason TEXT,
                result_message_id INTEGER,
                source_type TEXT NOT NULL DEFAULT 'user',
                task_timeout_secs INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(owner_id) REFERENCES kois(id) ON DELETE CASCADE,
                FOREIGN KEY(pool_session_id) REFERENCES pool_sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(claimed_by) REFERENCES kois(id) ON DELETE SET NULL
            );
            CREATE INDEX IF NOT EXISTS idx_koi_todos_owner ON koi_todos(owner_id);
            CREATE INDEX IF NOT EXISTS idx_koi_todos_status ON koi_todos(status);
        ",
        );
        // Migrate existing koi_todos table with new columns
        for col in &[
            "ALTER TABLE koi_todos ADD COLUMN claimed_by TEXT",
            "ALTER TABLE koi_todos ADD COLUMN claimed_at TEXT",
            "ALTER TABLE koi_todos ADD COLUMN depends_on TEXT",
            "ALTER TABLE koi_todos ADD COLUMN blocked_reason TEXT",
            "ALTER TABLE koi_todos ADD COLUMN result_message_id INTEGER",
            "ALTER TABLE koi_todos ADD COLUMN source_type TEXT NOT NULL DEFAULT 'user'",
            "ALTER TABLE koi_todos ADD COLUMN task_timeout_secs INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE koi_todos ADD COLUMN git_branch TEXT",
            "ALTER TABLE koi_todos ADD COLUMN integration_status TEXT NOT NULL DEFAULT 'none'",
        ] {
            let _ = self.conn.execute(col, []);
        }

        // Chat Pool sessions and messages
        let _ = self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS pool_sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                org_spec TEXT NOT NULL DEFAULT '',
                task_timeout_secs INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS pool_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pool_session_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                content TEXT NOT NULL,
                msg_type TEXT DEFAULT 'text',
                metadata TEXT DEFAULT '{}',
                todo_id TEXT,
                reply_to_message_id INTEGER,
                event_type TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(pool_session_id) REFERENCES pool_sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(todo_id) REFERENCES koi_todos(id) ON DELETE SET NULL,
                FOREIGN KEY(reply_to_message_id) REFERENCES pool_messages(id) ON DELETE SET NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pool_messages_session ON pool_messages(pool_session_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_pool_messages_todo ON pool_messages(todo_id);
        ");
        // Migrate existing pool_messages table with new columns
        for col in &[
            "ALTER TABLE pool_messages ADD COLUMN todo_id TEXT",
            "ALTER TABLE pool_messages ADD COLUMN reply_to_message_id INTEGER",
            "ALTER TABLE pool_messages ADD COLUMN event_type TEXT",
        ] {
            let _ = self.conn.execute(col, []);
        }
        // Migrate pool_sessions with org_spec
        let _ = self.conn.execute(
            "ALTER TABLE pool_sessions ADD COLUMN org_spec TEXT NOT NULL DEFAULT ''",
            [],
        );
        // Migrate pool_sessions with status, last_active_at, project_dir
        for col in &[
            "ALTER TABLE pool_sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
            "ALTER TABLE pool_sessions ADD COLUMN last_active_at TEXT",
            "ALTER TABLE pool_sessions ADD COLUMN project_dir TEXT DEFAULT NULL",
            "ALTER TABLE pool_sessions ADD COLUMN task_timeout_secs INTEGER NOT NULL DEFAULT 0",
            // Phase 0 — system-level IM notifications: track which (if
            // any) IM conversation originally requested this pool so the
            // heartbeat / decision flow can fan out beyond the desktop UI.
            "ALTER TABLE pool_sessions ADD COLUMN origin_im_binding_key TEXT",
            "ALTER TABLE pool_sessions ADD COLUMN team_id TEXT",
            "ALTER TABLE pool_sessions ADD COLUMN workflow_run_id TEXT",
        ] {
            let _ = self.conn.execute(col, []);
        }
        let _ = self.conn.execute(
            "ALTER TABLE kois ADD COLUMN role TEXT NOT NULL DEFAULT ''",
            [],
        );

        // Tiny key/value table for one-shot migration guards and similar
        // bookkeeping that must persist across restarts.
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS app_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        );

        // Pool membership — explicit project team roster. A Koi must be a
        // member of a pool before it can be assigned work there. Previously
        // the UI treated *every* Koi as a participant of *every* pool; this
        // table makes membership an explicit, per-project relationship.
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pool_members (
                pool_session_id TEXT NOT NULL,
                koi_id TEXT NOT NULL,
                joined_at TEXT NOT NULL,
                PRIMARY KEY (pool_session_id, koi_id),
                FOREIGN KEY (pool_session_id) REFERENCES pool_sessions(id) ON DELETE CASCADE,
                FOREIGN KEY (koi_id) REFERENCES kois(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_pool_members_pool ON pool_members(pool_session_id);
            CREATE INDEX IF NOT EXISTS idx_pool_members_koi ON pool_members(koi_id);
            ",
        );
        // One-time backfill: pre-existing pools have no explicit roster, so
        // derive their initial membership from the distinct Koi owners that
        // already have todos in the pool. Pools without any historical Koi
        // todo start empty — the user adds members via the UI. We do NOT
        // default to "all Kois" anymore. Guarded by a meta flag so it runs
        // exactly once even though this migration block executes on every
        // startup.
        if !self.meta_flag_is_set("pool_members_backfilled") {
            let _ = self.backfill_pool_members_from_todos();
            let _ = self.set_meta_flag("pool_members_backfilled");
        }

        // Phase 0 — scheduled tasks can opt into multi-target delivery
        // (UI + IM bindings + IM sessions). Stored as JSON so the
        // shape evolves without further migrations.
        let _ = self.conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN notify_targets_json TEXT",
            [],
        );

        // Phase 0 — pending decision requests. Lays the groundwork for
        // Phase 4's two-way IM decision flow; nothing is written today.
        let _ = self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS pending_decisions (
                id TEXT PRIMARY KEY,
                session_id TEXT,
                pool_id TEXT,
                origin TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL,
                message TEXT NOT NULL,
                options_json TEXT,
                targets_json TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                response_json TEXT,
                expires_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (pool_id) REFERENCES pool_sessions(id) ON DELETE SET NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pending_decisions_status
                ON pending_decisions(status, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_pending_decisions_pool
                ON pending_decisions(pool_id);
            CREATE INDEX IF NOT EXISTS idx_pending_decisions_session
                ON pending_decisions(session_id);
            ",
        );

        if let Ok(normalized) = self.normalize_identifier_references() {
            if normalized > 0 {
                tracing::info!(
                    "Database startup: normalized {} legacy identifier references",
                    normalized
                );
            }
        }

        // Clean up orphaned Koi / pool records from older schemas that lacked FK constraints.
        let _ = self.conn.execute_batch(
            "
            DELETE FROM koi_todos
            WHERE owner_id NOT IN (SELECT id FROM kois);

            DELETE FROM koi_todos
            WHERE pool_session_id IS NOT NULL
              AND pool_session_id NOT IN (SELECT id FROM pool_sessions);

            UPDATE koi_todos
            SET claimed_by = NULL, claimed_at = NULL
            WHERE claimed_by IS NOT NULL
              AND claimed_by NOT IN (SELECT id FROM kois);

            DELETE FROM pool_messages
            WHERE pool_session_id NOT IN (SELECT id FROM pool_sessions);

            UPDATE pool_messages
            SET todo_id = NULL
            WHERE todo_id IS NOT NULL
              AND todo_id NOT IN (SELECT id FROM koi_todos);

            UPDATE pool_messages
            SET reply_to_message_id = NULL
            WHERE reply_to_message_id IS NOT NULL
              AND reply_to_message_id NOT IN (SELECT id FROM pool_messages);
        ",
        );

        // Triggers provide cascade-like cleanup for existing databases created before FK support.
        let _ = self.conn.execute_batch(
            "
            CREATE TRIGGER IF NOT EXISTS trg_pool_sessions_delete_cleanup
            AFTER DELETE ON pool_sessions
            BEGIN
                DELETE FROM pool_messages WHERE pool_session_id = OLD.id;
                DELETE FROM koi_todos WHERE pool_session_id = OLD.id;
            END;

            CREATE TRIGGER IF NOT EXISTS trg_kois_delete_cleanup
            AFTER DELETE ON kois
            BEGIN
                DELETE FROM koi_todos WHERE owner_id = OLD.id;
                UPDATE koi_todos SET claimed_by = NULL, claimed_at = NULL WHERE claimed_by = OLD.id;
                DELETE FROM memories WHERE owner_id = OLD.id;
            END;

            CREATE TRIGGER IF NOT EXISTS trg_koi_todos_delete_cleanup
            AFTER DELETE ON koi_todos
            BEGIN
                UPDATE pool_messages SET todo_id = NULL WHERE todo_id = OLD.id;
            END;
        ",
        );

        // Memory isolation: add owner_id, scope_type, scope_id to memories
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'xiaoheng'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN scope_type TEXT NOT NULL DEFAULT 'private'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN scope_id TEXT NOT NULL DEFAULT 'xiaoheng'",
            [],
        );
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memories_owner ON memories(owner_id);
             CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope_type, scope_id);",
        );

        // Memory project tagging: allows private memories to be tagged with the project they were
        // created in, enabling project-priority search while still falling back to cross-project skills.
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN project_scope_id TEXT", // NULL = cross-project skill/preference
            [],
        );
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memories_project_scope ON memories(owner_id, scope_type, project_scope_id);"
        );

        // Phase 4d (v2 rolling-summary plan): richer memory schema so
        // long-term memory can act as a personalised codebook (Phase
        // 4b) conditioning L2. See agent::summary_worker docs.
        //
        // - kind            → short typology ("fact" | "decision" |
        //                     "preference" | "error_learned" | …)
        //                     mirroring the structured rolling summary
        //                     shards. Default "fact".
        // - evidence_session_id / evidence_tool_use_id → FEC anchors
        //   that point back to the concrete tool exchange that
        //   produced this memory. NULL when the memory predates Phase
        //   4d or was hand-saved by the user.
        // - last_seen_at    → timestamp of the most recent re-observation;
        //                     driven by save_memory's confidence-bump
        //                     path. Defaults to created_at on insertion.
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN kind TEXT NOT NULL DEFAULT 'fact'",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN evidence_session_id TEXT",
            [],
        );
        let _ = self.conn.execute(
            "ALTER TABLE memories ADD COLUMN evidence_tool_use_id TEXT",
            [],
        );
        let _ = self
            .conn
            .execute("ALTER TABLE memories ADD COLUMN last_seen_at TEXT", []);
        // Backfill last_seen_at for legacy rows.
        let _ = self.conn.execute(
            "UPDATE memories SET last_seen_at = updated_at WHERE last_seen_at IS NULL",
            [],
        );
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(owner_id, kind);\
             CREATE INDEX IF NOT EXISTS idx_memories_evidence_session ON memories(evidence_session_id);"
        );

        // One-time deduplication: remove duplicate messages caused by a previous bug where
        // persist_agent_turn saved the full message history (including already-stored messages).
        // Keep the earliest row (lowest rowid) for each (session_id, role, content) group.
        let _ = self.conn.execute_batch("
            DELETE FROM messages
            WHERE rowid NOT IN (
                SELECT MIN(rowid)
                FROM messages
                GROUP BY session_id, role, content, COALESCE(tool_calls_json,''), COALESCE(tool_results_json,'')
            );
        ");

        // Seed default skills if empty
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))?;
        if count == 0 {
            self.seed_skills()?;
        }

        Ok(())
    }

    fn seed_skills(&self) -> Result<()> {
        let skills = vec![
            (
                "web-search",
                "Web Search",
                "Search the web for information",
                true,
                "🔍",
            ),
            (
                "shell",
                "Shell / PowerShell",
                "Execute shell commands via PowerShell",
                true,
                "💻",
            ),
            (
                "file-ops",
                "File Operations",
                "Read, write and edit files",
                true,
                "📁",
            ),
            (
                "uia",
                "Windows UI Automation",
                "Control Windows desktop apps via UIA",
                true,
                "🖥️",
            ),
            (
                "screen-vision",
                "Screen Vision",
                "Screenshot + Vision AI fallback",
                true,
                "👁️",
            ),
            (
                "scheduled-tasks",
                "Scheduled Tasks",
                "Recurring automated tasks",
                true,
                "⏰",
            ),
            (
                "docx",
                "Word Document",
                "Generate .docx documents",
                true,
                "📄",
            ),
            (
                "xlsx",
                "Excel Spreadsheet",
                "Generate .xlsx spreadsheets",
                true,
                "📊",
            ),
        ];
        for (id, name, desc, enabled, icon) in skills {
            self.conn.execute(
                "INSERT OR IGNORE INTO skills (id, name, description, enabled, icon, config) VALUES (?1, ?2, ?3, ?4, ?5, '{}')",
                params![id, name, desc, enabled as i64, icon],
            )?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Sessions
    // ------------------------------------------------------------------

    pub fn create_session(&self, title: Option<&str>) -> Result<Session> {
        self.create_session_with_source(title, "chat")
    }

    pub fn create_session_with_source(&self, title: Option<&str>, source: &str) -> Result<Session> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO sessions (id, title, status, source, created_at, updated_at, message_count) VALUES (?1, ?2, 'idle', ?3, ?4, ?4, 0)",
            params![id, title, source, now_str],
        )?;
        Ok(Session {
            id,
            title: title.map(String::from),
            status: "idle".into(),
            source: source.to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            rolling_summary: String::new(),
            rolling_summary_version: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            last_compacted_at: None,
            workspace_root: None,
            pinned_at: None,
            archived_at: None,
            pool_session_id: None,
            policy_mode: None,
            allow_outside_workspace: None,
        })
    }

    pub fn pin_session(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET pinned_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn unpin_session(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET pinned_at = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn archive_session(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn restore_session(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET archived_at = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn list_archived_sessions(&self, limit: i64, offset: i64) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, status, COALESCE(source, 'chat'), created_at, updated_at, message_count, \
                    COALESCE(rolling_summary, ''), COALESCE(rolling_summary_version, 0), \
                    COALESCE(total_input_tokens, 0), COALESCE(total_output_tokens, 0), last_compacted_at, \
                    workspace_root, pinned_at, archived_at, pool_session_id, policy_mode, \
                    allow_outside_workspace \
             FROM sessions WHERE archived_at IS NOT NULL ORDER BY archived_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], map_session_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, status, COALESCE(source, 'chat'), created_at, updated_at, message_count, \
                    COALESCE(rolling_summary, ''), COALESCE(rolling_summary_version, 0), \
                    COALESCE(total_input_tokens, 0), COALESCE(total_output_tokens, 0), last_compacted_at, \
                    workspace_root, pinned_at, archived_at, pool_session_id, policy_mode, \
                    allow_outside_workspace \
             FROM sessions WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], map_session_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn ensure_fixed_session(
        &self,
        session_id: &str,
        title: &str,
        source: &str,
    ) -> Result<Session> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status, source, created_at, updated_at, message_count) VALUES (?1, ?2, 'idle', ?3, ?4, ?4, 0)",
            params![session_id, title, source, now_str],
        )?;
        self.get_session(session_id)?
            .ok_or_else(|| anyhow::anyhow!("Session '{}' missing after ensure", session_id))
    }

    /// Idempotent: create a session with a fixed `id` for IM routing.
    /// If it already exists, return it as-is (updating `updated_at` is skipped
    /// to preserve chronological ordering in the session list).
    pub fn ensure_im_session(
        &self,
        session_id: &str,
        title: &str,
        source: &str,
    ) -> Result<Session> {
        self.ensure_fixed_session(session_id, title, source)
    }

    pub fn get_im_session_binding(&self, binding_key: &str) -> Result<Option<ImSessionBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             WHERE binding_key = ?1",
        )?;
        let mut rows = stmt.query_map(params![binding_key], map_im_session_binding_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_im_session_binding_by_session(
        &self,
        session_id: &str,
        channel: &str,
    ) -> Result<Option<ImSessionBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             WHERE session_id = ?1 AND channel = ?2
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![session_id, channel], map_im_session_binding_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn find_im_session_binding_for_channel_recipient(
        &self,
        channel: &str,
        recipient: &str,
    ) -> Result<Option<ImSessionBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             WHERE channel = ?1 AND (latest_reply_target = ?2 OR peer_id = ?2)
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![channel, recipient], map_im_session_binding_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_im_session_bindings(
        &self,
        channel: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ImSessionBinding>> {
        let limit = limit.max(1) as i64;
        let sql_with_channel =
            "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             WHERE channel = ?1
             ORDER BY updated_at DESC
             LIMIT ?2";
        let sql_all = "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             ORDER BY updated_at DESC
             LIMIT ?1";

        let mut out = Vec::new();
        if let Some(channel) = channel {
            let mut stmt = self.conn.prepare(sql_with_channel)?;
            let rows = stmt.query_map(params![channel, limit], map_im_session_binding_row)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare(sql_all)?;
            let rows = stmt.query_map(params![limit], map_im_session_binding_row)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn upsert_im_session_binding(
        &self,
        input: &ImSessionBindingUpsert,
    ) -> Result<ImSessionBinding> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO im_session_bindings (
                binding_key, channel, external_conversation_key, session_id, peer_id,
                peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                created_at, updated_at, last_inbound_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?11)
             ON CONFLICT(binding_key) DO UPDATE SET
                channel = excluded.channel,
                external_conversation_key = excluded.external_conversation_key,
                session_id = excluded.session_id,
                peer_id = excluded.peer_id,
                peer_name = excluded.peer_name,
                is_group = excluded.is_group,
                group_name = excluded.group_name,
                latest_reply_target = excluded.latest_reply_target,
                routing_state_json = excluded.routing_state_json,
                updated_at = excluded.updated_at,
                last_inbound_at = excluded.last_inbound_at",
            params![
                input.binding_key,
                input.channel,
                input.external_conversation_key,
                input.session_id,
                input.peer_id,
                input.peer_name,
                input.is_group,
                input.group_name,
                input.latest_reply_target,
                input.routing_state_json,
                now,
            ],
        )?;
        self.get_im_session_binding(&input.binding_key)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "IM session binding '{}' missing after upsert",
                    input.binding_key
                )
            })
    }

    pub fn list_sessions(&self, limit: i64, offset: i64, include_mobile: bool) -> Result<Vec<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, status, COALESCE(source, 'chat'), created_at, updated_at, message_count, \
                    COALESCE(rolling_summary, ''), COALESCE(rolling_summary_version, 0), \
                    COALESCE(total_input_tokens, 0), COALESCE(total_output_tokens, 0), last_compacted_at, \
                    workspace_root, pinned_at, archived_at, pool_session_id, policy_mode, \
                    allow_outside_workspace \
             FROM sessions WHERE archived_at IS NULL AND COALESCE(source, 'chat') != 'mobile_executor' \
               AND (?2 = 1 OR COALESCE(source, 'chat') != 'mobile') ORDER BY \
                    CASE WHEN pinned_at IS NOT NULL THEN 0 ELSE 1 END, \
                    COALESCE(pinned_at, updated_at) DESC, updated_at DESC LIMIT ?1 OFFSET ?3"
        )?;
        let rows = stmt.query_map(params![limit, include_mobile as i32, offset], map_session_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn mobile_session_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM sessions WHERE source = 'mobile'")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>().map_err(Into::into)
    }

    pub fn delete_all_mobile_sessions(&self) -> Result<usize> {
        let ids = self.mobile_session_ids()?;
        for id in &ids { self.delete_session(id)?; }
        Ok(ids.len())
    }

    /// Sessions currently in `running` status (agent loop active).
    pub fn count_running_sessions(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status = 'running'",
            [],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    /// True when no session is running and the latest session activity is older than `min_hours`.
    pub fn is_system_idle_for_hours(&self, min_hours: u32) -> Result<bool> {
        if self.count_running_sessions()? > 0 {
            return Ok(false);
        }
        let latest: Option<String> = self
            .conn
            .query_row("SELECT MAX(updated_at) FROM sessions", [], |r| r.get(0))
            .unwrap_or(None);
        let Some(latest) = latest else {
            return Ok(true);
        };
        let parsed = latest
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());
        Ok(Utc::now().signed_duration_since(parsed).num_hours() >= min_hours as i64)
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM agent_checkpoints WHERE session_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM audit_log WHERE session_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now, id],
        )?;
        Ok(())
    }

    /// Set or clear the per-session workspace override.
    /// Pass `None` to revert to the global workspace_root.
    pub fn set_session_workspace(&self, id: &str, workspace_root: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET workspace_root = ?1, updated_at = ?2 WHERE id = ?3",
            params![workspace_root, now, id],
        )?;
        Ok(())
    }

    pub fn set_session_pool_id(&self, id: &str, pool_session_id: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET pool_session_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![pool_session_id, now, id],
        )?;
        Ok(())
    }

    /// Set or clear the per-session tool policy override.
    /// Pass `None` to inherit the global policy_mode from settings.
    pub fn set_session_policy_mode(&self, id: &str, policy_mode: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET policy_mode = ?1, updated_at = ?2 WHERE id = ?3",
            params![policy_mode, now, id],
        )?;
        Ok(())
    }

    /// Pass `None` to inherit the global allow_outside_workspace from settings.
    pub fn set_session_allow_outside_workspace(
        &self,
        id: &str,
        allow_outside_workspace: Option<bool>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let value: Option<i64> = allow_outside_workspace.map(i64::from);
        self.conn.execute(
            "UPDATE sessions SET allow_outside_workspace = ?1, updated_at = ?2 WHERE id = ?3",
            params![value, now, id],
        )?;
        Ok(())
    }

    pub fn update_session_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id],
        )?;
        Ok(())
    }

    pub fn get_session_context_state(&self, id: &str) -> Result<Option<SessionContextState>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, COALESCE(rolling_summary, ''), COALESCE(rolling_summary_version, 0), \
                    COALESCE(total_input_tokens, 0), COALESCE(total_output_tokens, 0), last_compacted_at \
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |r| {
            Ok(SessionContextState {
                session_id: r.get(0)?,
                rolling_summary: r.get(1)?,
                rolling_summary_version: r.get(2)?,
                total_input_tokens: r.get(3)?,
                total_output_tokens: r.get(4)?,
                last_compacted_at: parse_optional_datetime(r.get::<_, Option<String>>(5)?),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_session_usage_totals(
        &self,
        session_id: &str,
        input_delta: u32,
        output_delta: u32,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions
             SET total_input_tokens = total_input_tokens + ?1,
                 total_output_tokens = total_output_tokens + ?2
             WHERE id = ?3",
            params![i64::from(input_delta), i64::from(output_delta), session_id],
        )?;
        Ok(())
    }

    pub fn update_session_rolling_summary(
        &self,
        session_id: &str,
        summary: &str,
        version: i64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions
             SET rolling_summary = ?1,
                 rolling_summary_version = ?2,
                 last_compacted_at = ?3,
                 updated_at = ?3
             WHERE id = ?4",
            params![summary, version, now, session_id],
        )?;
        Ok(())
    }

    /// Read the persisted `state_frame_json` blob for a session, if any.
    /// Returns `Ok(None)` when the column is NULL / empty / the session
    /// does not exist — callers should treat this as "no frame yet".
    pub fn get_session_state_frame_json(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state_frame_json FROM sessions WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![session_id], |r| r.get::<_, Option<String>>(0))?;
        match rows.next().transpose()? {
            Some(Some(raw)) if !raw.trim().is_empty() => Ok(Some(raw)),
            _ => Ok(None),
        }
    }

    /// Persist (or clear with `None`) the state frame JSON for a session.
    /// Also bumps `updated_at` so cross-device sync stays monotonic.
    pub fn update_session_state_frame_json(
        &self,
        session_id: &str,
        frame_json: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET state_frame_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![frame_json, now, session_id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Messages
    // ------------------------------------------------------------------

    pub fn append_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
    ) -> Result<ChatMessage> {
        self.append_message_full_with_context(session_id, role, content, None, None, None, None)
    }

    /// Import a mobile message under its original ID so repeated syncs are idempotent.
    pub fn import_mobile_message(
        &self,
        session_id: &str,
        message_id: &str,
        role: &str,
        content: &str,
    ) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO messages (id, session_id, role, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![message_id, session_id, role, content, now],
        )?;
        if inserted > 0 {
            self.conn.execute(
                "UPDATE sessions SET message_count = message_count + 1, updated_at = ?1 WHERE id = ?2",
                params![now, session_id],
            )?;
        }
        Ok(inserted > 0)
    }

    /// Store the server sequence for a shared message. A local message with the
    /// same ID is acknowledged in place; content conflicts are never ignored.
    pub fn import_shared_message(
        &self, session_id: &str, message_id: &str, role: &str, content: &str,
        seq: u64, origin_device_id: &str,
    ) -> Result<bool> {
        let seq = i64::try_from(seq).map_err(|_| anyhow::anyhow!("shared sequence out of range"))?;
        let inserted = self.import_mobile_message(session_id, message_id, role, content)?;
        let changed = self.conn.execute(
            "UPDATE messages SET server_seq = ?1, origin_device_id = ?2
             WHERE id = ?3 AND session_id = ?4 AND role = ?5 AND content = ?6
               AND (server_seq IS NULL OR server_seq = ?1)",
            params![seq, origin_device_id, message_id, session_id, role, content],
        )?;
        if changed == 0 { return Err(anyhow::anyhow!("shared message ID conflicts with local content")); }
        Ok(inserted)
    }

    pub fn get_unshared_mobile_messages(&self, session_id: &str, limit: i64) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id,session_id,role,content,created_at,tool_calls_json,tool_results_json,turn_index,context_json
             FROM messages WHERE session_id = ?1 AND server_seq IS NULL
               AND role IN ('user','assistant') ORDER BY rowid ASC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![session_id, limit], map_chat_message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn record_shared_desktop_turn(&self, session_id: &str, turn_id: &str, initiating_message_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO mobile_desktop_turns (turn_id,session_id,initiating_message_id,created_at)
             VALUES (?1,?2,?3,?4)",
            params![turn_id, session_id, initiating_message_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn pending_shared_desktop_turns(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id,turn_id,initiating_message_id FROM mobile_desktop_turns WHERE completed = 0 ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn has_shared_assistant_after(&self, session_id: &str, initiating_message_id: &str) -> Result<bool> {
        let exists: i64 = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages answer JOIN messages prompt ON prompt.id = ?2
             WHERE answer.session_id = ?1 AND answer.role = 'assistant'
               AND answer.server_seq IS NOT NULL AND answer.rowid > prompt.rowid)",
            params![session_id, initiating_message_id], |row| row.get(0),
        )?;
        Ok(exists != 0)
    }

    pub fn complete_shared_desktop_turn(&self, turn_id: &str) -> Result<()> {
        self.conn.execute("UPDATE mobile_desktop_turns SET completed = 1 WHERE turn_id = ?1", params![turn_id])?;
        Ok(())
    }

    pub fn append_message_with_context(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        context_json: Option<&str>,
    ) -> Result<ChatMessage> {
        self.append_message_full_with_context(
            session_id,
            role,
            content,
            None,
            None,
            None,
            context_json,
        )
    }

    /// Persist a message with optional tool call data and turn index.
    /// `tool_calls_json`: JSON array of ToolUse blocks (for assistant messages).
    /// `tool_results_json`: JSON array of ToolResult blocks (for user/tool messages).
    /// `turn_index`: 1-based conversation turn counter.
    pub fn append_message_full(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        tool_calls_json: Option<&str>,
        tool_results_json: Option<&str>,
        turn_index: Option<i64>,
    ) -> Result<ChatMessage> {
        self.append_message_full_with_context(
            session_id,
            role,
            content,
            tool_calls_json,
            tool_results_json,
            turn_index,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn append_message_full_with_context(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        tool_calls_json: Option<&str>,
        tool_results_json: Option<&str>,
        turn_index: Option<i64>,
        context_json: Option<&str>,
    ) -> Result<ChatMessage> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO messages (id, session_id, role, content, created_at, tool_calls_json, tool_results_json, turn_index, context_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, session_id, role, content, now_str, tool_calls_json, tool_results_json, turn_index, context_json],
        )?;
        // Update session message count and updated_at
        self.conn.execute(
            "UPDATE sessions SET message_count = message_count + 1, updated_at = ?1 WHERE id = ?2",
            params![now_str, session_id],
        )?;
        Ok(ChatMessage {
            id,
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            created_at: now,
            tool_calls_json: tool_calls_json.map(|s| s.to_string()),
            tool_results_json: tool_results_json.map(|s| s.to_string()),
            turn_index,
            context_json: context_json.map(str::to_string),
        })
    }

    /// Delete all messages for a session that belong to a specific turn index.
    /// This is used to clean up "in-progress" agent messages when a turn is
    /// cancelled mid-flight, preventing incomplete tool-call/tool-result pairs
    /// from polluting the conversation history.
    pub fn delete_messages_by_turn_index(
        &self,
        session_id: &str,
        turn_index: i64,
    ) -> Result<usize> {
        let count = self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND turn_index = ?2",
            params![session_id, turn_index],
        )?;
        Ok(count)
    }

    /// Recompute the message_count for a session by counting rows in messages.
    /// Called after deleting messages so the session counter stays accurate.
    pub fn recompute_session_message_count(&self, session_id: &str) -> Result<()> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE sessions SET message_count = ?1, updated_at = ?2 WHERE id = ?3",
            params![count, now, session_id],
        )?;
        Ok(())
    }

    pub fn get_messages(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ChatMessage>> {
        // Sort by rowid (insert order) rather than created_at to be robust against
        // system clock drift. See `get_messages_latest` for full rationale.
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tool_calls_json, tool_results_json, turn_index, context_json \
             FROM messages WHERE session_id = ?1 ORDER BY COALESCE(server_seq, 9223372036854775807), rowid ASC LIMIT ?2 OFFSET ?3"
        )?;
        let rows = stmt.query_map(params![session_id, limit, offset], map_chat_message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch `limit` messages older than the newest `offset` messages, in chronological order.
    /// Used for scroll-up pagination: skip the newest `offset` rows, return the next `limit` older rows.
    /// Sorts by rowid (insert order) for clock-skew robustness.
    pub fn get_messages_older(
        &self,
        session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tool_calls_json, tool_results_json, turn_index, context_json \
             FROM ( \
               SELECT id, session_id, role, content, created_at, tool_calls_json, tool_results_json, turn_index, context_json, server_seq, rowid \
               FROM messages WHERE session_id = ?1 \
               ORDER BY COALESCE(server_seq, 9223372036854775807) DESC, rowid DESC LIMIT ?2 OFFSET ?3 \
             ) ORDER BY COALESCE(server_seq, 9223372036854775807), rowid ASC",
        )?;
        let rows = stmt.query_map(params![session_id, limit, offset], map_chat_message_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch the latest `limit` messages for a session, ordered chronologically (oldest first).
    /// Unlike `get_messages`, this always includes the most recent messages rather than the oldest,
    /// which is critical for building LLM context when a session has many messages.
    ///
    /// Sorts by `rowid DESC` (insert order) rather than `created_at DESC` to be robust
    /// against system clock drift. Once a message gets a future-dated timestamp (e.g., due to
    /// timezone confusion or clock adjustment), `created_at`-based sorting would cause that
    /// message to permanently dominate the "latest" position, causing the agent to repeatedly
    /// see stale conversation context. `rowid` reflects true SQLite insert order and is immune
    /// to clock skew.
    pub fn get_messages_latest(&self, session_id: &str, limit: i64) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, created_at, tool_calls_json, tool_results_json, turn_index, context_json \
             FROM messages WHERE session_id = ?1 ORDER BY COALESCE(server_seq, 9223372036854775807) DESC, rowid DESC LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![session_id, limit], map_chat_message_row)?;
        let mut msgs: Vec<ChatMessage> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        msgs.reverse(); // Return in chronological order (oldest first)
        Ok(msgs)
    }

    pub fn append_turn_execution_event(
        &self,
        session_id: &str,
        turn_id: &str,
        sequence: i64,
        event_type: &str,
        payload_json: &str,
    ) -> Result<TurnExecutionEventRecord> {
        const MAX_EVENTS_PER_TURN: i64 = 500;
        const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
        if sequence < 1 {
            return Err(anyhow::anyhow!("execution event sequence must be positive"));
        }
        if payload_json.len() > MAX_PAYLOAD_BYTES {
            return Err(anyhow::anyhow!("execution event payload exceeds 16 KiB"));
        }
        let payload = serde_json::from_str::<Value>(payload_json)
            .context("execution event payload must be valid JSON")?;
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM turn_execution_events WHERE turn_id = ?1",
            params![turn_id],
            |row| row.get(0),
        )?;
        if count >= MAX_EVENTS_PER_TURN {
            return Err(anyhow::anyhow!("execution event limit reached for turn"));
        }

        let record = TurnExecutionEventRecord {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            sequence,
            event_type: event_type.to_string(),
            payload,
            created_at: Utc::now(),
        };
        self.conn.execute(
            "INSERT INTO turn_execution_events (id, session_id, turn_id, sequence, event_type, payload_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.session_id,
                record.turn_id,
                record.sequence,
                record.event_type,
                payload_json,
                record.created_at.to_rfc3339(),
            ],
        )?;
        Ok(record)
    }

    pub fn list_turn_execution_events(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<TurnExecutionEventRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, turn_id, sequence, event_type, payload_json, created_at FROM ( \
                SELECT rowid AS event_order, id, session_id, turn_id, sequence, event_type, payload_json, created_at \
                FROM turn_execution_events WHERE session_id = ?1 \
                ORDER BY rowid DESC LIMIT ?2 \
             ) ORDER BY event_order ASC",
        )?;
        let rows = stmt.query_map(params![session_id, limit.clamp(1, 500)], |row| {
            let payload_json = row.get::<_, String>(5)?;
            Ok(TurnExecutionEventRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                turn_id: row.get(2)?,
                sequence: row.get(3)?,
                event_type: row.get(4)?,
                payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
                created_at: parse_datetime(row.get::<_, String>(6)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ------------------------------------------------------------------
    // Session artifacts
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn add_session_artifact(
        &self,
        session_id: &str,
        name: &str,
        artifact_type: &str,
        uri: Option<&str>,
        content_summary: &str,
        source_tool: Option<&str>,
        tool_use_id: Option<&str>,
        metadata_json: Option<&str>,
    ) -> Result<SessionArtifact> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(anyhow::anyhow!("artifact name is required"));
        }
        if self.get_session(session_id)?.is_none() {
            return Err(anyhow::anyhow!("session '{}' not found", session_id));
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let normalized_type = artifact_type.trim().to_ascii_lowercase();
        let normalized_type = if normalized_type.is_empty() {
            "file".to_string()
        } else {
            normalized_type
        };
        self.conn.execute(
            "INSERT INTO session_artifacts \
             (id, session_id, name, artifact_type, uri, content_summary, source_tool, tool_use_id, metadata_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                session_id,
                trimmed_name,
                normalized_type,
                uri,
                content_summary.trim(),
                source_tool,
                tool_use_id,
                metadata_json,
                now_str
            ],
        )?;

        Ok(SessionArtifact {
            id,
            session_id: session_id.to_string(),
            name: trimmed_name.to_string(),
            artifact_type: normalized_type,
            uri: uri.map(ToOwned::to_owned),
            content_summary: content_summary.trim().to_string(),
            source_tool: source_tool.map(ToOwned::to_owned),
            tool_use_id: tool_use_id.map(ToOwned::to_owned),
            metadata_json: metadata_json.map(ToOwned::to_owned),
            created_at: now,
        })
    }

    pub fn list_session_artifacts(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionArtifact>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, name, artifact_type, uri, content_summary, source_tool, tool_use_id, metadata_json, created_at \
             FROM session_artifacts WHERE session_id = ?1 ORDER BY rowid DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![session_id, limit.max(1)], |r| {
            Ok(SessionArtifact {
                id: r.get(0)?,
                session_id: r.get(1)?,
                name: r.get(2)?,
                artifact_type: r.get(3)?,
                uri: r.get(4)?,
                content_summary: r.get(5)?,
                source_tool: r.get(6)?,
                tool_use_id: r.get(7)?,
                metadata_json: r.get(8)?,
                created_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// List artifacts across all sessions, newest first (for global "My Files").
    pub fn list_all_artifacts(&self, limit: i64) -> Result<Vec<SessionArtifact>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, name, artifact_type, uri, content_summary, source_tool, tool_use_id, metadata_json, created_at \
             FROM session_artifacts ORDER BY rowid DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit.max(1)], |r| {
            Ok(SessionArtifact {
                id: r.get(0)?,
                session_id: r.get(1)?,
                name: r.get(2)?,
                artifact_type: r.get(3)?,
                uri: r.get(4)?,
                content_summary: r.get(5)?,
                source_tool: r.get(6)?,
                tool_use_id: r.get(7)?,
                metadata_json: r.get(8)?,
                created_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ------------------------------------------------------------------
    // Memories
    // ------------------------------------------------------------------

    pub fn list_memories(&self) -> Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, category, confidence, source_session_id, owner_id, scope_type, scope_id, project_scope_id, created_at, updated_at, kind, evidence_session_id, evidence_tool_use_id, last_seen_at \
             FROM memories ORDER BY COALESCE(last_seen_at, updated_at, created_at) DESC, updated_at DESC, confidence DESC"
        )?;
        let rows = stmt.query_map([], Self::map_memory)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// List memories filtered by owner_id.
    pub fn list_memories_for_owner(&self, owner_id: &str) -> Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, category, confidence, source_session_id, owner_id, scope_type, scope_id, project_scope_id, created_at, updated_at, kind, evidence_session_id, evidence_tool_use_id, last_seen_at \
             FROM memories WHERE owner_id = ?1 ORDER BY COALESCE(last_seen_at, updated_at, created_at) DESC, updated_at DESC, confidence DESC"
        )?;
        let rows = stmt.query_map(params![owner_id], Self::map_memory)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn map_memory(r: &rusqlite::Row) -> rusqlite::Result<Memory> {
        Ok(Memory {
            id: r.get(0)?,
            content: r.get(1)?,
            category: r.get(2)?,
            confidence: r.get(3)?,
            source_session_id: r.get(4)?,
            memory_type: "personal".to_string(),
            owner_id: r
                .get::<_, String>(5)
                .unwrap_or_else(|_| "xiaoheng".to_string()),
            scope_type: r
                .get::<_, String>(6)
                .unwrap_or_else(|_| "private".to_string()),
            scope_id: r
                .get::<_, String>(7)
                .unwrap_or_else(|_| "xiaoheng".to_string()),
            project_scope_id: r.get::<_, Option<String>>(8).unwrap_or(None),
            created_at: r
                .get::<_, String>(9)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: r
                .get::<_, String>(10)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            kind: r
                .get::<_, String>(11)
                .unwrap_or_else(|_| "fact".to_string()),
            evidence_session_id: r.get::<_, Option<String>>(12).unwrap_or(None),
            evidence_tool_use_id: r.get::<_, Option<String>>(13).unwrap_or(None),
            last_seen_at: r
                .get::<_, Option<String>>(14)
                .ok()
                .flatten()
                .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
        })
    }

    /// Save a memory with dedup: if a very similar memory already exists (same category + owner,
    /// high content overlap), update it instead of creating a duplicate.
    ///
    /// `project_scope_id`: for private memories, the pool_session_id where this memory was created.
    /// NULL means it is a cross-project skill or preference (visible in all projects as a fallback).
    #[allow(clippy::too_many_arguments)]
    pub fn save_memory(
        &self,
        content: &str,
        category: &str,
        confidence: f64,
        source_session_id: Option<&str>,
        owner_id: &str,
        scope_type: &str,
        scope_id: &str,
        project_scope_id: Option<&str>,
    ) -> Result<Memory> {
        self.save_memory_structured(
            content,
            category,
            confidence,
            source_session_id,
            owner_id,
            scope_type,
            scope_id,
            project_scope_id,
            MemorySaveExtras::default(),
        )
    }

    /// Phase 4d — structured save path.
    ///
    /// Extends the legacy `save_memory` with the new v2 fields:
    /// - `kind`: structured rolling summary shard typology.
    /// - `evidence_session_id` / `evidence_tool_use_id`: FEC anchors.
    ///
    /// Also implements the **confidence-bump on re-observation**
    /// protocol from the v2 plan: when `find_similar_memory` matches,
    /// we raise `confidence` by `0.1` (capped at `1.0`) instead of
    /// replacing it with `max(new, existing)`. This implements the
    /// Dictionary-coding / AEP view — repeat observations of the same
    /// fact reinforce belief.
    ///
    /// `last_seen_at` is always refreshed to "now"; `updated_at` is
    /// refreshed **only** when content changes (preserving UI ordering).
    #[allow(clippy::too_many_arguments)]
    pub fn save_memory_structured(
        &self,
        content: &str,
        category: &str,
        confidence: f64,
        source_session_id: Option<&str>,
        owner_id: &str,
        scope_type: &str,
        scope_id: &str,
        project_scope_id: Option<&str>,
        extras: MemorySaveExtras,
    ) -> Result<Memory> {
        let kind = extras.kind.unwrap_or_else(|| "fact".to_string());
        if let Some(existing) = self.find_similar_memory(content, category, owner_id)? {
            let now = Utc::now();
            let now_str = now.to_rfc3339();
            // Confidence bump: +0.1 per re-observation, capped at 1.0.
            // Also honours a lower bound of `confidence` for callers
            // that pass an explicitly high belief.
            let bumped = (existing.confidence + 0.1).min(1.0);
            let new_confidence = bumped.max(confidence);
            let content_changed = existing.content != content;
            // Always refresh last_seen_at; refresh updated_at only when
            // the canonical content actually changes.
            if content_changed {
                self.conn.execute(
                    "UPDATE memories SET content = ?1, confidence = ?2, updated_at = ?3, last_seen_at = ?3, embedding = NULL WHERE id = ?4",
                    params![content, new_confidence, now_str, existing.id],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE memories SET confidence = ?1, last_seen_at = ?2 WHERE id = ?3",
                    params![new_confidence, now_str, existing.id],
                )?;
            }
            tracing::info!(
                "Memory dedup: updated existing memory {} (confidence {:.2} -> {:.2})",
                existing.id,
                existing.confidence,
                new_confidence
            );
            return Ok(Memory {
                id: existing.id,
                content: content.to_string(),
                category: category.to_string(),
                confidence: new_confidence,
                source_session_id: existing.source_session_id,
                memory_type: existing.memory_type,
                owner_id: existing.owner_id,
                scope_type: existing.scope_type,
                scope_id: existing.scope_id,
                project_scope_id: existing.project_scope_id,
                kind: existing.kind,
                evidence_session_id: existing.evidence_session_id,
                evidence_tool_use_id: existing.evidence_tool_use_id,
                last_seen_at: Some(now),
                created_at: existing.created_at,
                updated_at: if content_changed {
                    now
                } else {
                    existing.updated_at
                },
            });
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO memories (id, content, category, confidence, source_session_id, owner_id, scope_type, scope_id, project_scope_id, kind, evidence_session_id, evidence_tool_use_id, last_seen_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?13)",
            params![
                id,
                content,
                category,
                confidence,
                source_session_id,
                owner_id,
                scope_type,
                scope_id,
                project_scope_id,
                kind,
                extras.evidence_session_id.as_deref(),
                extras.evidence_tool_use_id.as_deref(),
                now_str,
            ],
        )?;
        Ok(Memory {
            id,
            content: content.to_string(),
            category: category.to_string(),
            confidence,
            source_session_id: source_session_id.map(String::from),
            memory_type: "personal".to_string(),
            owner_id: owner_id.to_string(),
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            project_scope_id: project_scope_id.map(String::from),
            kind,
            evidence_session_id: extras.evidence_session_id,
            evidence_tool_use_id: extras.evidence_tool_use_id,
            last_seen_at: Some(now),
            created_at: now,
            updated_at: now,
        })
    }

    /// Find a memory in the same category+owner that has high content overlap with the given text.
    /// Uses word-level Jaccard similarity (threshold: 0.6).
    fn find_similar_memory(
        &self,
        content: &str,
        category: &str,
        owner_id: &str,
    ) -> Result<Option<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, category, confidence, source_session_id, owner_id, scope_type, scope_id, project_scope_id, created_at, updated_at \
             FROM memories WHERE category = ?1 AND owner_id = ?2 ORDER BY updated_at DESC LIMIT 50"
        )?;
        let rows = stmt.query_map(params![category, owner_id], Self::map_memory)?;

        let new_words: std::collections::HashSet<&str> = content.split_whitespace().collect();
        if new_words.is_empty() {
            return Ok(None);
        }

        for mem in rows.flatten() {
            let existing_words: std::collections::HashSet<&str> =
                mem.content.split_whitespace().collect();
            if existing_words.is_empty() {
                continue;
            }
            let intersection = new_words.intersection(&existing_words).count();
            let union = new_words.union(&existing_words).count();
            let jaccard = intersection as f64 / union as f64;
            if jaccard >= 0.6 {
                return Ok(Some(mem));
            }
        }
        Ok(None)
    }

    pub fn delete_memory(&self, id: &str) -> Result<()> {
        let owner = self.conn.query_row("SELECT owner_id FROM memories WHERE id = ?1", params![id], |row| row.get::<_, String>(0)).optional()?;
        let tx = self.conn.unchecked_transaction()?;
        if owner.as_deref() == Some("xiaoheng") {
            tx.execute(
                "INSERT OR REPLACE INTO memory_tombstones (id, deleted_at) VALUES (?1, ?2)",
                params![id, Utc::now().to_rfc3339()],
            )?;
        }
        tx.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_memories(&self) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO memory_tombstones (id, deleted_at) SELECT id, ?1 FROM memories WHERE owner_id = 'xiaoheng'",
            params![Utc::now().to_rfc3339()],
        )?;
        tx.execute("DELETE FROM memories", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn export_memory_changes(&self) -> Result<Vec<MemoryCloudChange>> {
        let mut changes = self.list_memories_for_owner("xiaoheng")?
            .into_iter()
            .map(|memory| MemoryCloudChange {
                id: memory.id.clone(),
                updated_at: memory.last_seen_at.unwrap_or(memory.updated_at).max(memory.updated_at),
                memory: Some(memory),
            })
            .collect::<Vec<_>>();
        let mut statement = self.conn.prepare("SELECT id, deleted_at FROM memory_tombstones")?;
        let tombstones = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for tombstone in tombstones {
            let (id, deleted_at) = tombstone?;
            if let Ok(updated_at) = deleted_at.parse() {
                changes.push(MemoryCloudChange { id, updated_at, memory: None });
            }
        }
        Ok(changes)
    }

    pub fn ensure_memory_sync_account(&self, user_id: &str) -> Result<()> {
        if user_id.trim().is_empty() {
            return Err(anyhow::anyhow!("missing cloud account id"));
        }
        let bound = self.conn.query_row(
            "SELECT user_id FROM memory_sync_identity WHERE singleton = 1", [],
            |row| row.get::<_, String>(0),
        );
        match bound {
            Ok(existing) if existing == user_id => Ok(()),
            Ok(_) => Err(anyhow::anyhow!("本机记忆已绑定其他账号；请使用独立的 Windows 用户资料或先完整清理本机数据")),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.conn.execute(
                    "INSERT INTO memory_sync_identity (singleton, user_id) VALUES (1, ?1)",
                    params![user_id],
                )?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn apply_memory_changes(&self, changes: &[MemoryCloudChange]) -> Result<()> {
        let mut local = self.export_memory_changes()?.into_iter()
            .map(|change| (change.id.clone(), change))
            .collect::<std::collections::HashMap<_, _>>();
        let tx = self.conn.unchecked_transaction()?;
        for change in changes {
            let existing_owner = tx.query_row(
                "SELECT owner_id FROM memories WHERE id = ?1", params![change.id],
                |row| row.get::<_, String>(0),
            ).optional()?;
            if existing_owner.as_deref().is_some_and(|owner| owner != "xiaoheng") {
                return Err(anyhow::anyhow!("memory cloud id belongs to another owner"));
            }
            if let Some(memory) = &change.memory {
                let latest_observation = memory.last_seen_at.unwrap_or(memory.updated_at).max(memory.updated_at);
                if memory.owner_id != "xiaoheng" || memory.id != change.id
                    || (change.updated_at != memory.updated_at && change.updated_at != latest_observation) {
                    return Err(anyhow::anyhow!("invalid memory cloud record"));
                }
            }
            let current = local.get(&change.id);
            if current.as_ref().is_some_and(|existing| {
                existing.updated_at > change.updated_at
                    || (existing.updated_at == change.updated_at && existing.memory.is_none())
            }) {
                continue;
            }
            match &change.memory {
                None => {
                    tx.execute("DELETE FROM memories WHERE id = ?1 AND owner_id = 'xiaoheng'", params![change.id])?;
                    tx.execute(
                        "INSERT OR REPLACE INTO memory_tombstones (id, deleted_at) VALUES (?1, ?2)",
                        params![change.id, change.updated_at.to_rfc3339()],
                    )?;
                }
                Some(memory) => {
                    tx.execute("DELETE FROM memory_tombstones WHERE id = ?1", params![change.id])?;
                    tx.execute(
                        "INSERT INTO memories (id, content, category, confidence, source_session_id, owner_id, scope_type, scope_id, project_scope_id, kind, evidence_session_id, evidence_tool_use_id, last_seen_at, created_at, updated_at) \
                         VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, NULL, NULL, ?10, ?11, ?12) \
                         ON CONFLICT(id) DO UPDATE SET content=excluded.content, category=excluded.category, confidence=excluded.confidence, scope_type=excluded.scope_type, scope_id=excluded.scope_id, project_scope_id=excluded.project_scope_id, kind=excluded.kind, last_seen_at=excluded.last_seen_at, updated_at=excluded.updated_at, embedding=NULL",
                        params![memory.id, memory.content, memory.category, memory.confidence,
                            memory.owner_id, memory.scope_type, memory.scope_id, memory.project_scope_id, memory.kind,
                            memory.last_seen_at.map(|time| time.to_rfc3339()),
                            memory.created_at.to_rfc3339(), memory.updated_at.to_rfc3339()],
                    )?;
                }
            }
            local.insert(change.id.clone(), change.clone());
        }
        tx.commit()?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Vector / Embedding support
    // ------------------------------------------------------------------

    /// Store a floating-point embedding for an existing memory row.
    pub fn store_embedding(&self, memory_id: &str, embedding: &[f32]) -> Result<()> {
        let bytes = super::vector::embedding_to_bytes(embedding);
        self.conn.execute(
            "UPDATE memories SET embedding = ?1 WHERE id = ?2",
            params![bytes, memory_id],
        )?;
        Ok(())
    }

    /// Retrieve all memories that have an embedding stored.
    pub fn list_memories_with_embeddings(&self) -> Result<Vec<(Memory, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, category, confidence, source_session_id, \
             owner_id, scope_type, scope_id, project_scope_id, created_at, updated_at, embedding, \
             kind, evidence_session_id, evidence_tool_use_id, last_seen_at \
             FROM memories WHERE embedding IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            let embedding_bytes: Vec<u8> = r.get(11)?;
            Ok((
                Memory {
                    id: r.get(0)?,
                    content: r.get(1)?,
                    category: r.get(2)?,
                    confidence: r.get(3)?,
                    source_session_id: r.get(4)?,
                    memory_type: "personal".to_string(),
                    owner_id: r
                        .get::<_, String>(5)
                        .unwrap_or_else(|_| "xiaoheng".to_string()),
                    scope_type: r
                        .get::<_, String>(6)
                        .unwrap_or_else(|_| "private".to_string()),
                    scope_id: r
                        .get::<_, String>(7)
                        .unwrap_or_else(|_| "xiaoheng".to_string()),
                    project_scope_id: r.get::<_, Option<String>>(8).unwrap_or(None),
                    created_at: r
                        .get::<_, String>(9)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: r
                        .get::<_, String>(10)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                    kind: r
                        .get::<_, String>(12)
                        .unwrap_or_else(|_| "fact".to_string()),
                    evidence_session_id: r.get::<_, Option<String>>(13).unwrap_or(None),
                    evidence_tool_use_id: r.get::<_, Option<String>>(14).unwrap_or(None),
                    last_seen_at: r
                        .get::<_, Option<String>>(15)
                        .ok()
                        .flatten()
                        .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
                },
                embedding_bytes,
            ))
        })?;
        let pairs: rusqlite::Result<Vec<(Memory, Vec<u8>)>> = rows.collect();
        let pairs = pairs?;
        Ok(pairs
            .into_iter()
            .map(|(m, bytes)| {
                let embedding = super::vector::bytes_to_embedding(&bytes);
                (m, embedding)
            })
            .collect())
    }

    /// Scan memories with vector similarity against a query embedding.
    /// Returns (Memory, cosine_score) pairs sorted by descending score.
    pub fn search_by_embedding(
        &self,
        query_vec: &[f32],
        top_k: usize,
    ) -> Result<Vec<(Memory, f32)>> {
        let all = self.list_memories_with_embeddings()?;
        let mut scored: Vec<(Memory, f32)> = all
            .into_iter()
            .map(|(m, emb)| {
                let score = super::vector::cosine_similarity(query_vec, &emb);
                (m, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        Ok(scored)
    }

    /// Full-text search using FTS5. Returns (memory_id, bm25_score) pairs.
    pub fn fts_search(&self, query: &str, top_k: usize) -> Result<Vec<(String, f32)>> {
        // Sanitise query for FTS5: escape special chars
        let safe_query = query.replace('"', "\"\"").replace(['*', '^'], "");
        let fts_query = format!("\"{}\"", safe_query);

        let mut stmt = self.conn.prepare(
            "SELECT m.id, bm25(memories_fts) AS score \
             FROM memories_fts \
             JOIN memories m ON m.rowid = memories_fts.rowid \
             WHERE memories_fts MATCH ?1 \
             ORDER BY score \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, top_k as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })?;
        let results: rusqlite::Result<Vec<_>> = rows.collect();
        // bm25 returns negative scores; negate to make higher = better
        Ok(results?.into_iter().map(|(id, s)| (id, -s)).collect())
    }

    // ------------------------------------------------------------------
    // Skills
    // ------------------------------------------------------------------

    pub fn list_skills(&self) -> Result<Vec<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, enabled, icon, config FROM skills ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Skill {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                enabled: r.get::<_, i64>(3)? != 0,
                icon: r.get(4)?,
                config: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn set_skill_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET enabled = ?1 WHERE id = ?2",
            params![enabled as i64, id],
        )?;
        Ok(())
    }

    /// Remove a skill record from the DB by ID.
    pub fn delete_skill(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM skills WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Insert or update a skill record in the DB.
    /// Uses the skill name (lowercased, sanitised) as the ID.
    /// If a record with the same ID already exists it is updated in-place.
    pub fn upsert_skill(&self, id: &str, name: &str, description: &str, icon: &str) -> Result<()> {
        self.upsert_skill_with_config(id, name, description, icon, None)
    }

    /// Like [`upsert_skill`] but preserves or sets the JSON `config` blob.
    pub fn upsert_skill_with_config(
        &self,
        id: &str,
        name: &str,
        description: &str,
        icon: &str,
        config: Option<&str>,
    ) -> Result<()> {
        let config_json = config.unwrap_or("{}");
        self.conn.execute(
            "INSERT INTO skills (id, name, description, enabled, icon, config) \
             VALUES (?1, ?2, ?3, 1, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
               name=excluded.name, \
               description=excluded.description, \
               icon=excluded.icon, \
               config=CASE WHEN excluded.config != '{}' THEN excluded.config ELSE skills.config END",
            params![id, name, description, icon, config_json],
        )?;
        Ok(())
    }

    pub fn update_skill_config(&self, id: &str, config: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET config = ?1 WHERE id = ?2",
            params![config, id],
        )?;
        Ok(())
    }

    pub fn get_skill(&self, id: &str) -> Result<Option<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, enabled, icon, config FROM skills WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(Skill {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                enabled: r.get::<_, i64>(3)? != 0,
                icon: r.get(4)?,
                config: r.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn insert_skill_revision(
        &self,
        skill_id: &str,
        session_id: Option<&str>,
        origin: &str,
        diff_summary: Option<&str>,
        content_before_hash: Option<&str>,
        content_after_hash: Option<&str>,
    ) -> Result<SkillRevision> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO skill_revisions \
             (id, skill_id, session_id, origin, diff_summary, content_before_hash, content_after_hash, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                skill_id,
                session_id,
                origin,
                diff_summary,
                content_before_hash,
                content_after_hash,
                now.to_rfc3339(),
            ],
        )?;
        Ok(SkillRevision {
            id,
            skill_id: skill_id.to_string(),
            session_id: session_id.map(String::from),
            origin: origin.to_string(),
            diff_summary: diff_summary.map(String::from),
            content_before_hash: content_before_hash.map(String::from),
            content_after_hash: content_after_hash.map(String::from),
            created_at: now,
        })
    }

    pub fn list_skill_revisions_for_skill(
        &self,
        skill_id: &str,
        limit: i64,
    ) -> Result<Vec<SkillRevision>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, skill_id, session_id, origin, diff_summary, content_before_hash, content_after_hash, created_at \
             FROM skill_revisions WHERE skill_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![skill_id, limit], |r| {
            Ok(SkillRevision {
                id: r.get(0)?,
                skill_id: r.get(1)?,
                session_id: r.get(2)?,
                origin: r.get(3)?,
                diff_summary: r.get(4)?,
                content_before_hash: r.get(5)?,
                content_after_hash: r.get(6)?,
                created_at: r
                    .get::<_, String>(7)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_skill_revisions_for_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SkillRevision>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, skill_id, session_id, origin, diff_summary, content_before_hash, content_after_hash, created_at \
             FROM skill_revisions WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![session_id, limit], |r| {
            Ok(SkillRevision {
                id: r.get(0)?,
                skill_id: r.get(1)?,
                session_id: r.get(2)?,
                origin: r.get(3)?,
                diff_summary: r.get(4)?,
                content_before_hash: r.get(5)?,
                content_after_hash: r.get(6)?,
                created_at: r
                    .get::<_, String>(7)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn ensure_skill_usage(&self, skill_id: &str, created_by: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO skill_usage (skill_id, created_by) VALUES (?1, ?2) \
             ON CONFLICT(skill_id) DO NOTHING",
            params![skill_id, created_by],
        )?;
        Ok(())
    }

    pub fn get_skill_usage(&self, skill_id: &str) -> Result<Option<SkillUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT skill_id, view_count, use_count, patch_count, last_used_at, last_patched_at, state, pinned, created_by \
             FROM skill_usage WHERE skill_id = ?1",
        )?;
        let mut rows = stmt.query(params![skill_id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(Self::row_to_skill_usage(r)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_skill_usage(&self) -> Result<Vec<SkillUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT skill_id, view_count, use_count, patch_count, last_used_at, last_patched_at, state, pinned, created_by \
             FROM skill_usage ORDER BY use_count DESC, view_count DESC",
        )?;
        let rows = stmt.query_map([], Self::row_to_skill_usage)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn bump_skill_view(&self, skill_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO skill_usage (skill_id, view_count, last_used_at) VALUES (?1, 1, ?2) \
             ON CONFLICT(skill_id) DO UPDATE SET view_count = view_count + 1, last_used_at = ?2",
            params![skill_id, now],
        )?;
        Ok(())
    }

    pub fn bump_skill_use(&self, skill_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO skill_usage (skill_id, use_count, last_used_at) VALUES (?1, 1, ?2) \
             ON CONFLICT(skill_id) DO UPDATE SET use_count = use_count + 1, last_used_at = ?2",
            params![skill_id, now],
        )?;
        Ok(())
    }

    pub fn bump_skill_patch(&self, skill_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO skill_usage (skill_id, patch_count, last_patched_at) VALUES (?1, 1, ?2) \
             ON CONFLICT(skill_id) DO UPDATE SET patch_count = patch_count + 1, last_patched_at = ?2",
            params![skill_id, now],
        )?;
        Ok(())
    }

    pub fn set_skill_usage_state(&self, skill_id: &str, state: &str) -> Result<()> {
        self.ensure_skill_usage(skill_id, None)?;
        self.conn.execute(
            "UPDATE skill_usage SET state = ?1 WHERE skill_id = ?2",
            params![state, skill_id],
        )?;
        Ok(())
    }

    pub fn set_skill_pinned(&self, skill_id: &str, pinned: bool) -> Result<()> {
        self.ensure_skill_usage(skill_id, None)?;
        self.conn.execute(
            "UPDATE skill_usage SET pinned = ?1 WHERE skill_id = ?2",
            params![pinned as i64, skill_id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Scheduled tasks
    // ------------------------------------------------------------------

    pub fn list_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, cron_expression, task_prompt, notify_targets_json, status, last_run_status, run_count, last_run_at, next_run_at, created_at FROM scheduled_tasks ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ScheduledTask {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                cron_expression: r.get(3)?,
                task_prompt: r.get(4)?,
                notify_targets_json: r.get(5)?,
                status: r.get(6)?,
                last_run_status: r.get(7)?,
                run_count: r.get(8)?,
                last_run_at: r.get::<_, Option<String>>(9)?.and_then(|s| s.parse().ok()),
                next_run_at: r.get::<_, Option<String>>(10)?.and_then(|s| s.parse().ok()),
                created_at: r
                    .get::<_, String>(11)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_task(&self, id: &str) -> Result<Option<ScheduledTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, cron_expression, task_prompt, notify_targets_json, status, last_run_status, run_count, last_run_at, next_run_at, created_at FROM scheduled_tasks WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], |r| {
            Ok(ScheduledTask {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                cron_expression: r.get(3)?,
                task_prompt: r.get(4)?,
                notify_targets_json: r.get(5)?,
                status: r.get(6)?,
                last_run_status: r.get(7)?,
                run_count: r.get(8)?,
                last_run_at: r.get::<_, Option<String>>(9)?.and_then(|s| s.parse().ok()),
                next_run_at: r.get::<_, Option<String>>(10)?.and_then(|s| s.parse().ok()),
                created_at: r
                    .get::<_, String>(11)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn create_task(
        &self,
        name: &str,
        description: Option<&str>,
        cron_expression: &str,
        task_prompt: &str,
        notify_targets_json: Option<&str>,
    ) -> Result<ScheduledTask> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO scheduled_tasks (id, name, description, cron_expression, task_prompt, notify_targets_json, status, run_count, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', 0, ?7)",
            params![id, name, description, cron_expression, task_prompt, notify_targets_json, now_str],
        )?;
        Ok(ScheduledTask {
            id,
            name: name.to_string(),
            description: description.map(String::from),
            cron_expression: cron_expression.to_string(),
            task_prompt: task_prompt.to_string(),
            notify_targets_json: notify_targets_json.map(String::from),
            status: "active".into(),
            last_run_status: None,
            run_count: 0,
            last_run_at: None,
            next_run_at: None,
            created_at: now,
        })
    }

    pub fn update_task(
        &self,
        id: &str,
        name: Option<&str>,
        cron_expression: Option<&str>,
        task_prompt: Option<&str>,
        notify_targets_json: Option<&str>,
        status: Option<&str>,
    ) -> Result<()> {
        if let Some(n) = name {
            self.conn.execute(
                "UPDATE scheduled_tasks SET name = ?1 WHERE id = ?2",
                params![n, id],
            )?;
        }
        if let Some(c) = cron_expression {
            self.conn.execute(
                "UPDATE scheduled_tasks SET cron_expression = ?1 WHERE id = ?2",
                params![c, id],
            )?;
        }
        if let Some(p) = task_prompt {
            self.conn.execute(
                "UPDATE scheduled_tasks SET task_prompt = ?1 WHERE id = ?2",
                params![p, id],
            )?;
        }
        if let Some(targets) = notify_targets_json {
            self.conn.execute(
                "UPDATE scheduled_tasks SET notify_targets_json = ?1 WHERE id = ?2",
                params![targets, id],
            )?;
        }
        if let Some(s) = status {
            self.conn.execute(
                "UPDATE scheduled_tasks SET status = ?1 WHERE id = ?2",
                params![s, id],
            )?;
        }
        Ok(())
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn record_task_run(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE scheduled_tasks SET run_count = run_count + 1, last_run_at = ?1, last_run_status = 'running' WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Update the last_run_status for a task: "success", "failed", or "running".
    pub fn update_task_run_status(&self, id: &str, run_status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET last_run_status = ?1 WHERE id = ?2",
            params![run_status, id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Audit Log
    // ------------------------------------------------------------------

    pub fn append_audit(
        &self,
        session_id: &str,
        tool_name: &str,
        action: &str,
        input_summary: Option<&str>,
        result_summary: Option<&str>,
        is_error: bool,
    ) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO audit_log (id, session_id, timestamp, tool_name, action, input_summary, result_summary, is_error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, session_id, now, tool_name, action, input_summary, result_summary, is_error as i64],
        )?;
        Ok(())
    }

    pub fn get_audit_log(
        &self,
        session_id: Option<&str>,
        tool_name: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditEntry>> {
        let mut query = String::from(
            "SELECT id, session_id, timestamp, tool_name, action, input_summary, result_summary, is_error \
             FROM audit_log WHERE 1=1"
        );
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(sid) = session_id {
            query.push_str(&format!(" AND session_id = ?{}", bind_values.len() + 1));
            bind_values.push(sid.to_string());
        }
        if let Some(tool) = tool_name {
            query.push_str(&format!(" AND tool_name = ?{}", bind_values.len() + 1));
            bind_values.push(tool.to_string());
        }
        query.push_str(&format!(
            " ORDER BY timestamp DESC LIMIT {} OFFSET {}",
            limit, offset
        ));

        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(bind_values.iter()), |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                session_id: r.get(1)?,
                timestamp: r
                    .get::<_, String>(2)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                tool_name: r.get(3)?,
                action: r.get(4)?,
                input_summary: r.get(5)?,
                result_summary: r.get(6)?,
                is_error: r.get::<_, i64>(7)? != 0,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn insert_plan_snapshot(
        &self,
        session_id: &str,
        label: &str,
        items_json: &str,
    ) -> Result<PlanSnapshot> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO plan_snapshots (id, session_id, label, items_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, session_id, label, items_json, now_str],
        )?;
        Ok(PlanSnapshot {
            id,
            session_id: session_id.to_string(),
            label: label.to_string(),
            items_json: items_json.to_string(),
            created_at: now,
        })
    }

    pub fn list_plan_snapshots_for_session(
        &self,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<PlanSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, label, items_json, created_at \
             FROM plan_snapshots WHERE session_id = ?1 \
             ORDER BY created_at ASC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![session_id, limit], |r| {
            Ok(PlanSnapshot {
                id: r.get(0)?,
                session_id: r.get(1)?,
                label: r.get(2)?,
                items_json: r.get(3)?,
                created_at: parse_datetime(r.get(4)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Distinct session ids that appear in audit_log or plan_snapshots (recent first).
    pub fn list_activity_session_ids(&self, limit: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, MAX(ts) AS latest FROM ( \
                SELECT session_id, timestamp AS ts FROM audit_log \
                UNION ALL \
                SELECT session_id, created_at AS ts FROM plan_snapshots \
                UNION ALL \
                SELECT session_id, created_at AS ts FROM session_artifacts \
             ) GROUP BY session_id ORDER BY latest DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn clear_audit_log(&self, session_id: Option<&str>) -> Result<()> {
        if let Some(sid) = session_id {
            self.conn
                .execute("DELETE FROM audit_log WHERE session_id = ?1", params![sid])?;
        } else {
            self.conn.execute("DELETE FROM audit_log", [])?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Memory FTS & Embeddings
    // ------------------------------------------------------------------

    /// Full-text search across all memories (global, no owner filter).
    pub fn search_memories_fts(&self, query: &str, limit: i64) -> Result<Vec<Memory>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.content, m.category, m.confidence, m.source_session_id, \
             m.owner_id, m.scope_type, m.scope_id, m.project_scope_id, m.created_at, m.updated_at \
             FROM memories m \
             JOIN memories_fts f ON m.rowid = f.rowid \
             WHERE memories_fts MATCH ?1 \
             ORDER BY rank \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], Self::map_memory)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Scoped memory search: retrieves memories in priority order (4 layers) for the given owner.
    ///
    /// Layer 1: private memories tagged to the current project (project_scope_id = pool_session_id)
    /// Layer 2: private memories with no project tag (cross-project skills/preferences)
    /// Layer 3: project-shared memories (scope_type = 'project', scope_id = pool_session_id)
    /// Layer 4: global memories
    pub fn search_memories_scoped(
        &self,
        query: &str,
        owner_id: &str,
        pool_session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Memory>> {
        let mut results = Vec::new();

        let dedupe = |results: &Vec<Memory>| -> std::collections::HashSet<String> {
            results.iter().map(|m| m.id.clone()).collect()
        };

        // Layer 1: project-specific private memories (highest priority)
        if let Some(psid) = pool_session_id {
            let mut stmt = self.conn.prepare(
                "SELECT m.id, m.content, m.category, m.confidence, m.source_session_id, \
                 m.owner_id, m.scope_type, m.scope_id, m.project_scope_id, m.created_at, m.updated_at \
                 FROM memories m \
                 JOIN memories_fts f ON m.rowid = f.rowid \
                 WHERE memories_fts MATCH ?1 AND m.owner_id = ?2 AND m.scope_type = 'private' \
                 AND m.project_scope_id = ?3 \
                 ORDER BY rank \
                 LIMIT ?4"
            )?;
            let rows = stmt.query_map(params![query, owner_id, psid, limit], Self::map_memory)?;
            results.extend(rows.flatten());
        }

        // Layer 2: cross-project private memories (skills, preferences — project_scope_id IS NULL)
        {
            let remaining = (limit - results.len() as i64).max(0);
            if remaining > 0 {
                let mut stmt = self.conn.prepare(
                    "SELECT m.id, m.content, m.category, m.confidence, m.source_session_id, \
                     m.owner_id, m.scope_type, m.scope_id, m.project_scope_id, m.created_at, m.updated_at \
                     FROM memories m \
                     JOIN memories_fts f ON m.rowid = f.rowid \
                     WHERE memories_fts MATCH ?1 AND m.owner_id = ?2 AND m.scope_type = 'private' \
                     AND m.project_scope_id IS NULL \
                     ORDER BY rank \
                     LIMIT ?3"
                )?;
                let rows = stmt.query_map(params![query, owner_id, remaining], Self::map_memory)?;
                let seen = dedupe(&results);
                for mem in rows.flatten() {
                    if !seen.contains(&mem.id) {
                        results.push(mem);
                    }
                }
            }
        }

        // Layer 3: project-shared memories
        if let Some(psid) = pool_session_id {
            let remaining = (limit - results.len() as i64).max(0);
            if remaining > 0 {
                let mut stmt = self.conn.prepare(
                    "SELECT m.id, m.content, m.category, m.confidence, m.source_session_id, \
                     m.owner_id, m.scope_type, m.scope_id, m.project_scope_id, m.created_at, m.updated_at \
                     FROM memories m \
                     JOIN memories_fts f ON m.rowid = f.rowid \
                     WHERE memories_fts MATCH ?1 AND m.scope_type = 'project' AND m.scope_id = ?2 \
                     ORDER BY rank \
                     LIMIT ?3"
                )?;
                let rows = stmt.query_map(params![query, psid, remaining], Self::map_memory)?;
                let seen = dedupe(&results);
                for mem in rows.flatten() {
                    if !seen.contains(&mem.id) {
                        results.push(mem);
                    }
                }
            }
        }

        // Layer 4: global memories
        {
            let remaining = (limit - results.len() as i64).max(0);
            if remaining > 0 {
                let mut stmt = self.conn.prepare(
                    "SELECT m.id, m.content, m.category, m.confidence, m.source_session_id, \
                     m.owner_id, m.scope_type, m.scope_id, m.project_scope_id, m.created_at, m.updated_at \
                     FROM memories m \
                     JOIN memories_fts f ON m.rowid = f.rowid \
                     WHERE memories_fts MATCH ?1 AND m.scope_type = 'global' \
                     ORDER BY rank \
                     LIMIT ?2"
                )?;
                let rows = stmt.query_map(params![query, remaining], Self::map_memory)?;
                let seen = dedupe(&results);
                for mem in rows.flatten() {
                    if !seen.contains(&mem.id) {
                        results.push(mem);
                    }
                }
            }
        }

        results.truncate(limit as usize);
        Ok(results)
    }

    pub fn save_embedding_cache(&self, content_hash: &str, embedding: &[u8]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at) VALUES (?1, ?2, ?3)",
            params![content_hash, embedding, now],
        )?;
        Ok(())
    }

    pub fn get_embedding_cache(&self, content_hash: &str) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT embedding FROM embedding_cache WHERE content_hash = ?1")?;
        let mut rows = stmt.query_map(params![content_hash], |r| r.get::<_, Vec<u8>>(0))?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_memory_embedding(&self, id: &str, embedding: &[u8]) -> Result<()> {
        self.conn.execute(
            "UPDATE memories SET embedding = ?1 WHERE id = ?2",
            params![embedding, id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Agent Checkpoints
    // ------------------------------------------------------------------

    /// Upsert a checkpoint for the given session. Replaces any existing running checkpoint.
    pub fn upsert_checkpoint(
        &self,
        session_id: &str,
        iteration: usize,
        messages_json: &str,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        // Delete old running checkpoint for this session first
        self.conn.execute(
            "DELETE FROM agent_checkpoints WHERE session_id = ?1 AND status = 'running'",
            params![session_id],
        )?;
        self.conn.execute(
            "INSERT INTO agent_checkpoints (id, session_id, iteration, messages_json, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?5)",
            params![id, session_id, iteration as i64, messages_json, now],
        )?;
        Ok(id)
    }

    /// Mark a checkpoint as completed (success) or failed.
    pub fn finish_checkpoint(&self, session_id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE agent_checkpoints SET status = ?1, updated_at = ?2 \
             WHERE session_id = ?3 AND status = 'running'",
            params![status, now, session_id],
        )?;
        Ok(())
    }

    /// Load a pending (running) checkpoint for a session, if any.
    pub fn load_checkpoint(&self, session_id: &str) -> Result<Option<(usize, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT iteration, messages_json FROM agent_checkpoints \
             WHERE session_id = ?1 AND status = 'running' \
             ORDER BY updated_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![session_id], |r| {
            Ok((r.get::<_, i64>(0)? as usize, r.get::<_, String>(1)?))
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Prune stale checkpoints older than the given number of hours to keep the table small.
    pub fn prune_checkpoints(&self, older_than_hours: i64) -> Result<usize> {
        let cutoff = (Utc::now() - chrono::Duration::hours(older_than_hours)).to_rfc3339();
        let n = self.conn.execute(
            "DELETE FROM agent_checkpoints WHERE created_at < ?1",
            params![cutoff],
        )?;
        Ok(n)
    }

    // ------------------------------------------------------------------
    // Fish instances
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Task States
    // ------------------------------------------------------------------

    /// Get or create a task state for the given scope (session or scheduled_task).
    pub fn get_or_create_task_state(&self, scope_type: &str, scope_id: &str) -> Result<TaskState> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope_type, scope_id, goal, state_json, summary, status, version, created_at, updated_at \
             FROM task_states WHERE scope_type = ?1 AND scope_id = ?2 \
             ORDER BY updated_at DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(params![scope_type, scope_id], |r| {
            Ok(TaskState {
                id: r.get(0)?,
                scope_type: r.get(1)?,
                scope_id: r.get(2)?,
                goal: r.get(3)?,
                state_json: r.get(4)?,
                summary: r.get(5)?,
                status: r.get(6)?,
                version: r.get(7)?,
                created_at: r
                    .get::<_, String>(8)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;

        if let Some(existing) = rows.next().transpose()? {
            return Ok(existing);
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO task_states (id, scope_type, scope_id, goal, state_json, summary, status, version, created_at, updated_at) \
             VALUES (?1, ?2, ?3, '', '{}', '', 'active', 1, ?4, ?4)",
            params![id, scope_type, scope_id, now],
        )?;

        Ok(TaskState {
            id,
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            goal: String::new(),
            state_json: "{}".to_string(),
            summary: String::new(),
            status: "active".to_string(),
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    /// Update a task state with new goal, state_json, summary, and status.
    pub fn update_task_state(
        &self,
        id: &str,
        goal: Option<&str>,
        state_json: Option<&str>,
        summary: Option<&str>,
        status: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE task_states SET \
             goal = COALESCE(?2, goal), \
             state_json = COALESCE(?3, state_json), \
             summary = COALESCE(?4, summary), \
             status = COALESCE(?5, status), \
             version = version + 1, \
             updated_at = ?6 \
             WHERE id = ?1",
            params![id, goal, state_json, summary, status, now],
        )?;
        Ok(())
    }

    /// Load task state for a scope, if it exists.
    pub fn load_task_state(&self, scope_type: &str, scope_id: &str) -> Result<Option<TaskState>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope_type, scope_id, goal, state_json, summary, status, version, created_at, updated_at \
             FROM task_states WHERE scope_type = ?1 AND scope_id = ?2 \
             ORDER BY updated_at DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(params![scope_type, scope_id], |r| {
            Ok(TaskState {
                id: r.get(0)?,
                scope_type: r.get(1)?,
                scope_id: r.get(2)?,
                goal: r.get(3)?,
                state_json: r.get(4)?,
                summary: r.get(5)?,
                status: r.get(6)?,
                version: r.get(7)?,
                created_at: r
                    .get::<_, String>(8)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_recent_task_states(&self, limit: i64) -> Result<Vec<TaskState>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, scope_type, scope_id, goal, state_json, summary, status, version, created_at, updated_at \
             FROM task_states ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(TaskState {
                id: r.get(0)?,
                scope_type: r.get(1)?,
                scope_id: r.get(2)?,
                goal: r.get(3)?,
                state_json: r.get(4)?,
                summary: r.get(5)?,
                status: r.get(6)?,
                version: r.get(7)?,
                created_at: parse_datetime(r.get(8)?),
                updated_at: parse_datetime(r.get(9)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    // ------------------------------------------------------------------
    // Koi (persistent Agents)
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_koi(
        &self,
        name: &str,
        role: &str,
        icon: &str,
        color: &str,
        system_prompt: &str,
        description: &str,
        llm_provider_id: Option<&str>,
        max_iterations: u32,
        task_timeout_secs: u32,
    ) -> Result<crate::agent_core::models::KoiDefinition> {
        let name = normalize_koi_name(name)?;
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO kois (id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, llm_provider_id, max_iterations, task_timeout_secs) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'idle', ?8, ?8, ?9, ?10, ?11)",
            params![id, name, role, icon, color, system_prompt, description, now_str, llm_provider_id, max_iterations, task_timeout_secs],
        )?;
        Ok(crate::agent_core::models::KoiDefinition {
            id,
            name,
            role: role.to_string(),
            icon: icon.to_string(),
            color: color.to_string(),
            system_prompt: system_prompt.to_string(),
            description: description.to_string(),
            status: "idle".to_string(),
            created_at: now,
            updated_at: now,
            llm_provider_id: llm_provider_id.map(String::from),
            max_iterations,
            task_timeout_secs,
        })
    }

    /// Insert a Koi row with a caller-supplied primary key. Used by
    /// integration tests (and by seeding migrations that want a
    /// deterministic `piscis` row) to satisfy the `kois.id` foreign
    /// keys on `koi_todos` / `claimed_by` without going through the
    /// UUID-generating [`Database::create_koi`].
    ///
    /// `INSERT OR IGNORE` — calling twice is idempotent.
    pub fn upsert_koi_with_id(&self, id: &str, name: &str) -> Result<()> {
        let name = normalize_koi_name(name)?;
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO kois (id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, max_iterations, task_timeout_secs) \
             VALUES (?1, ?2, '', '', '', '', '', 'idle', ?3, ?3, 0, 0)",
            params![id, name, now],
        )?;
        Ok(())
    }

    pub fn ensure_starter_kois(&self) -> Result<Vec<crate::agent_core::models::KoiDefinition>> {
        if !self.list_kois()?.is_empty() {
            return Ok(Vec::new());
        }

        let mut created = Vec::new();
        for spec in crate::agent_core::models::STARTER_KOI_SPECS {
            created.push(self.create_koi(
                spec.name,
                spec.role,
                spec.icon,
                spec.color,
                spec.system_prompt,
                spec.description,
                None,
                0,
                0,
            )?);
        }
        Ok(created)
    }

    pub fn list_kois(&self) -> Result<Vec<crate::agent_core::models::KoiDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, llm_provider_id, max_iterations, task_timeout_secs \
             FROM kois ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::agent_core::models::KoiDefinition {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get::<_, String>(2).unwrap_or_default(),
                icon: r.get(3)?,
                color: r.get(4)?,
                system_prompt: r.get(5)?,
                description: r.get(6)?,
                status: r.get(7)?,
                created_at: r
                    .get::<_, String>(8)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                llm_provider_id: r.get(10)?,
                max_iterations: r.get::<_, u32>(11).unwrap_or(0),
                task_timeout_secs: r.get::<_, u32>(12).unwrap_or(0),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_koi(&self, id: &str) -> Result<Option<crate::agent_core::models::KoiDefinition>> {
        let koi_row =
            |r: &rusqlite::Row| -> rusqlite::Result<crate::agent_core::models::KoiDefinition> {
                Ok(crate::agent_core::models::KoiDefinition {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    role: r.get::<_, String>(2).unwrap_or_default(),
                    icon: r.get(3)?,
                    color: r.get(4)?,
                    system_prompt: r.get(5)?,
                    description: r.get(6)?,
                    status: r.get(7)?,
                    created_at: r
                        .get::<_, String>(8)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: r
                        .get::<_, String>(9)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                    llm_provider_id: r.get(10)?,
                    max_iterations: r.get::<_, u32>(11).unwrap_or(0),
                    task_timeout_secs: r.get::<_, u32>(12).unwrap_or(0),
                })
            };

        // Exact match first
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, llm_provider_id, max_iterations, task_timeout_secs \
             FROM kois WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], koi_row)?;
        if let Some(row) = rows.next() {
            return Ok(Some(row?));
        }

        // Prefix match fallback (for short IDs like "4b80c3e1")
        if id.len() >= 6 {
            let pattern = format!("{}%", id);
            let mut stmt2 = self.conn.prepare(
                "SELECT id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, llm_provider_id, max_iterations, task_timeout_secs \
                 FROM kois WHERE id LIKE ?1"
            )?;
            let matches: Vec<crate::agent_core::models::KoiDefinition> = stmt2
                .query_map(params![pattern], koi_row)?
                .filter_map(|r| r.ok())
                .collect();
            if matches.len() == 1 {
                return Ok(Some(matches.into_iter().next().unwrap()));
            }
        }

        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_koi(
        &self,
        id: &str,
        name: Option<&str>,
        role: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        system_prompt: Option<&str>,
        description: Option<&str>,
        llm_provider_id: Option<Option<&str>>,
        max_iterations: Option<u32>,
        task_timeout_secs: Option<u32>,
    ) -> Result<()> {
        let normalized_name = match name {
            Some(name) => Some(normalize_koi_name(name)?),
            None => None,
        };
        let now = Utc::now().to_rfc3339();
        // llm_provider_id: None = don't change, Some(None) = clear, Some(Some(v)) = set
        match llm_provider_id {
            None => {
                self.conn.execute(
                    "UPDATE kois SET \
                     name = COALESCE(?2, name), \
                     role = COALESCE(?3, role), \
                     icon = COALESCE(?4, icon), \
                     color = COALESCE(?5, color), \
                     system_prompt = COALESCE(?6, system_prompt), \
                     description = COALESCE(?7, description), \
                     max_iterations = COALESCE(?9, max_iterations), \
                     task_timeout_secs = COALESCE(?10, task_timeout_secs), \
                     updated_at = ?8 \
                     WHERE id = ?1",
                    params![
                        id,
                        normalized_name.as_deref(),
                        role,
                        icon,
                        color,
                        system_prompt,
                        description,
                        now,
                        max_iterations,
                        task_timeout_secs
                    ],
                )?;
            }
            Some(provider_id) => {
                self.conn.execute(
                    "UPDATE kois SET \
                     name = COALESCE(?2, name), \
                     role = COALESCE(?3, role), \
                     icon = COALESCE(?4, icon), \
                     color = COALESCE(?5, color), \
                     system_prompt = COALESCE(?6, system_prompt), \
                     description = COALESCE(?7, description), \
                     llm_provider_id = ?9, \
                     max_iterations = COALESCE(?10, max_iterations), \
                     task_timeout_secs = COALESCE(?11, task_timeout_secs), \
                     updated_at = ?8 \
                     WHERE id = ?1",
                    params![
                        id,
                        normalized_name.as_deref(),
                        role,
                        icon,
                        color,
                        system_prompt,
                        description,
                        now,
                        provider_id,
                        max_iterations,
                        task_timeout_secs
                    ],
                )?;
            }
        }
        Ok(())
    }

    pub fn update_koi_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE kois SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status, now],
        )?;
        Ok(())
    }

    pub fn find_koi_by_name(
        &self,
        name: &str,
    ) -> Result<Option<crate::agent_core::models::KoiDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, icon, color, system_prompt, description, status, created_at, updated_at, llm_provider_id, max_iterations, task_timeout_secs \
             FROM kois WHERE name = ?1 ORDER BY created_at ASC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(params![name], |r| {
            Ok(crate::agent_core::models::KoiDefinition {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get::<_, String>(2).unwrap_or_default(),
                icon: r.get(3)?,
                color: r.get(4)?,
                system_prompt: r.get(5)?,
                description: r.get(6)?,
                status: r.get(7)?,
                created_at: r
                    .get::<_, String>(8)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                llm_provider_id: r.get(10)?,
                max_iterations: r.get::<_, u32>(11).unwrap_or(0),
                task_timeout_secs: r.get::<_, u32>(12).unwrap_or(0),
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn resolve_koi_identifier(
        &self,
        value: &str,
    ) -> Result<Option<crate::agent_core::models::KoiDefinition>> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }
        if let Some(koi) = self.get_koi(value)? {
            return Ok(Some(koi));
        }
        let matches: Vec<crate::agent_core::models::KoiDefinition> = self
            .list_kois()?
            .into_iter()
            .filter(|k| k.name == value)
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.into_iter().next()),
            _ => Err(anyhow::anyhow!("Koi identifier '{}' is ambiguous", value)),
        }
    }

    /// Remove duplicate Koi entries, keeping the oldest one per name.
    /// Returns the number of duplicates removed.
    pub fn dedup_kois(&self) -> Result<usize> {
        let all = self.list_kois()?;
        let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut removed = 0usize;
        for koi in &all {
            if let Some(_kept_id) = seen.get(&koi.name) {
                self.delete_koi(&koi.id)?;
                removed += 1;
            } else {
                seen.insert(koi.name.clone(), koi.id.clone());
            }
        }
        Ok(removed)
    }

    pub fn delete_koi(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM kois WHERE id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM koi_todos WHERE owner_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM memories WHERE owner_id = ?1", params![id])?;
        Ok(())
    }

    /// Count memories belonging to a specific owner.
    pub fn count_memories_for_owner(&self, owner_id: &str) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE owner_id = ?1",
            params![owner_id],
            |r| r.get(0),
        )?;
        Ok(count)
    }

    // ------------------------------------------------------------------
    // Koi Todos (Board)
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_koi_todo(
        &self,
        owner_id: &str,
        title: &str,
        description: &str,
        priority: &str,
        assigned_by: &str,
        pool_session_id: Option<&str>,
        source_type: &str,
        depends_on: Option<&str>,
        task_timeout_secs: u32,
    ) -> Result<crate::agent_core::models::KoiTodo> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO koi_todos (id, owner_id, title, description, status, priority, assigned_by, pool_session_id, source_type, depends_on, task_timeout_secs, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 'todo', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![id, owner_id, title, description, priority, assigned_by, pool_session_id, source_type, depends_on, task_timeout_secs, now_str],
        )?;
        Ok(crate::agent_core::models::KoiTodo {
            id,
            owner_id: owner_id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            status: "todo".to_string(),
            priority: priority.to_string(),
            assigned_by: assigned_by.to_string(),
            pool_session_id: pool_session_id.map(String::from),
            claimed_by: None,
            claimed_at: None,
            depends_on: depends_on.map(String::from),
            blocked_reason: None,
            git_branch: None,
            integration_status: Some("none".into()),
            result_message_id: None,
            source_type: source_type.to_string(),
            task_timeout_secs,
            created_at: now,
            updated_at: now,
        })
    }

    const KOI_TODO_COLS: &'static str = "id, owner_id, title, description, status, priority, assigned_by, \
        pool_session_id, claimed_by, claimed_at, depends_on, blocked_reason, result_message_id, source_type, \
        task_timeout_secs, git_branch, integration_status, created_at, updated_at";

    pub fn list_koi_todos(
        &self,
        owner_id: Option<&str>,
    ) -> Result<Vec<crate::agent_core::models::KoiTodo>> {
        let sql = if owner_id.is_some() {
            format!(
                "SELECT {} FROM koi_todos WHERE owner_id = ?1 ORDER BY created_at DESC",
                Self::KOI_TODO_COLS
            )
        } else {
            format!(
                "SELECT {} FROM koi_todos ORDER BY created_at DESC",
                Self::KOI_TODO_COLS
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(oid) = owner_id {
            stmt.query_map(params![oid], Self::map_koi_todo)?
        } else {
            stmt.query_map([], Self::map_koi_todo)?
        };
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_koi_todo(&self, id: &str) -> Result<Option<crate::agent_core::models::KoiTodo>> {
        let sql = format!(
            "SELECT {} FROM koi_todos WHERE id = ?1",
            Self::KOI_TODO_COLS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id], Self::map_koi_todo)?;
        Ok(rows.next().transpose()?)
    }

    fn map_koi_todo(r: &rusqlite::Row) -> rusqlite::Result<crate::agent_core::models::KoiTodo> {
        Ok(crate::agent_core::models::KoiTodo {
            id: r.get(0)?,
            owner_id: r.get(1)?,
            title: r.get(2)?,
            description: r.get(3)?,
            status: r.get(4)?,
            priority: r.get(5)?,
            assigned_by: r.get(6)?,
            pool_session_id: r.get(7)?,
            claimed_by: r.get(8)?,
            claimed_at: r
                .get::<_, Option<String>>(9)?
                .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
            depends_on: r.get(10)?,
            blocked_reason: r.get(11)?,
            result_message_id: r.get(12)?,
            source_type: r
                .get::<_, String>(13)
                .unwrap_or_else(|_| "user".to_string()),
            task_timeout_secs: r.get::<_, u32>(14).unwrap_or(0),
            git_branch: r.get(15).ok(),
            integration_status: r.get(16).ok(),
            created_at: r
                .get::<_, String>(17)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: r
                .get::<_, String>(18)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        })
    }

    pub fn set_koi_todo_git_branch(&self, id: &str, branch: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET git_branch = ?2, integration_status = CASE WHEN integration_status IN ('merged', 'conflict') THEN integration_status ELSE 'none' END, updated_at = ?3 WHERE id = ?1",
            params![id, branch, now],
        )?;
        Ok(())
    }

    pub fn set_koi_todo_integration_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET integration_status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status, now],
        )?;
        Ok(())
    }

    pub fn set_koi_todo_integration_status_by_branch(
        &self,
        pool_session_id: &str,
        branch: &str,
        status: &str,
    ) -> Result<u32> {
        let now = Utc::now().to_rfc3339();
        let count = self.conn.execute(
            "UPDATE koi_todos SET integration_status = ?3, updated_at = ?4 WHERE pool_session_id = ?1 AND git_branch = ?2",
            params![pool_session_id, branch, status, now],
        )?;
        Ok(count as u32)
    }

    pub fn update_koi_todo(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        priority: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET \
             title = COALESCE(?2, title), \
             description = COALESCE(?3, description), \
             status = COALESCE(?4, status), \
             blocked_reason = CASE \
                WHEN COALESCE(?4, status) IN ('blocked', 'needs_review') THEN blocked_reason \
                ELSE NULL \
             END, \
             priority = COALESCE(?5, priority), \
             updated_at = ?6 \
             WHERE id = ?1",
            params![id, title, description, status, priority, now],
        )?;
        Ok(())
    }

    /// Claim a todo (a Koi starts working on it)
    pub fn claim_koi_todo(&self, id: &str, claimed_by: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET claimed_by = ?2, claimed_at = ?3, status = 'in_progress', blocked_reason = NULL, updated_at = ?3 WHERE id = ?1",
            params![id, claimed_by, now],
        )?;
        Ok(())
    }

    /// Atomic, conditional claim used to fence concurrent dispatchers.
    ///
    /// Succeeds (returns `true`) only when the todo is **not already
    /// `in_progress`** — i.e. no other turn is currently running it. The
    /// single-writer DB lock makes the check-and-set atomic, so when
    /// several activators (heartbeat, auto-pickup, bootstrap patrol,
    /// `assign_koi`) race on the same pending todo exactly one wins and
    /// the rest observe `false` and skip.
    pub fn try_claim_koi_todo(&self, id: &str, claimed_by: &str) -> Result<bool> {
        let now = Utc::now().to_rfc3339();
        let affected = self.conn.execute(
            "UPDATE koi_todos SET claimed_by = ?2, claimed_at = ?3, status = 'in_progress', \
             blocked_reason = NULL, updated_at = ?3 \
             WHERE id = ?1 AND status <> 'in_progress'",
            params![id, claimed_by, now],
        )?;
        Ok(affected == 1)
    }

    /// True when `koi_id` already owns a different `in_progress` todo.
    /// Used together with [`try_claim_koi_todo`] inside one write closure
    /// to enforce single-turn-per-Koi serialization across concurrent
    /// dispatch passes.
    pub fn koi_has_other_in_progress_todo(
        &self,
        koi_id: &str,
        except_todo_id: &str,
    ) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM koi_todos \
             WHERE owner_id = ?1 AND status = 'in_progress' AND id <> ?2",
            params![koi_id, except_todo_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// True when `koi_id` owns any `in_progress` todo. Used as the
    /// board-derived half of the "is this Koi busy?" decision so that
    /// status writes from different dispatch paths (kernel coordinator,
    /// desktop `call_koi`) don't clobber each other.
    pub fn koi_has_in_progress_todo(&self, koi_id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM koi_todos WHERE owner_id = ?1 AND status = 'in_progress'",
            params![koi_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Block a todo with a reason
    pub fn block_koi_todo(&self, id: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET status = 'blocked', blocked_reason = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, reason, now],
        )?;
        Ok(())
    }

    pub fn mark_koi_todo_needs_review(&self, id: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET status = 'needs_review', blocked_reason = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, reason, now],
        )?;
        Ok(())
    }

    pub fn cancel_koi_todo(&self, id: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET status = 'cancelled', blocked_reason = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, reason, now],
        )?;
        Ok(())
    }

    pub fn resume_koi_todo(&self, id: &str, claimed_by: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET status = 'in_progress', claimed_by = ?2, claimed_at = ?3, blocked_reason = NULL, updated_at = ?3 WHERE id = ?1",
            params![id, claimed_by, now],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replace_koi_todo(
        &self,
        original: &crate::agent_core::models::KoiTodo,
        new_owner_id: &str,
        title: &str,
        description: &str,
        assigned_by: &str,
        source_type: &str,
        reason: &str,
        task_timeout_secs: Option<u32>,
    ) -> Result<crate::agent_core::models::KoiTodo> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let replacement_reason = format!("[Replaced by {}] {}", id, reason.trim());
        let task_timeout_secs = task_timeout_secs.unwrap_or(original.task_timeout_secs);

        self.conn.execute_batch("BEGIN IMMEDIATE TRANSACTION")?;
        let result = (|| -> Result<()> {
            self.conn.execute(
                "INSERT INTO koi_todos (id, owner_id, title, description, status, priority, assigned_by, pool_session_id, source_type, depends_on, task_timeout_secs, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, 'todo', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    &id,
                    new_owner_id,
                    title,
                    description,
                    &original.priority,
                    assigned_by,
                    original.pool_session_id.as_deref(),
                    source_type,
                    Some(original.id.as_str()),
                    task_timeout_secs,
                    &now_str,
                ],
            )?;
            self.conn.execute(
                "UPDATE koi_todos SET status = 'cancelled', blocked_reason = ?2, updated_at = ?3 WHERE id = ?1",
                params![&original.id, replacement_reason, &now_str],
            )?;
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(crate::agent_core::models::KoiTodo {
                    id,
                    owner_id: new_owner_id.to_string(),
                    title: title.to_string(),
                    description: description.to_string(),
                    status: "todo".to_string(),
                    priority: original.priority.clone(),
                    assigned_by: assigned_by.to_string(),
                    pool_session_id: original.pool_session_id.clone(),
                    claimed_by: None,
                    claimed_at: None,
                    depends_on: Some(original.id.clone()),
                    blocked_reason: None,
                    git_branch: None,
                    integration_status: Some("none".into()),
                    result_message_id: None,
                    source_type: source_type.to_string(),
                    task_timeout_secs,
                    created_at: now,
                    updated_at: now,
                })
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// Complete a todo with a link to the result message
    pub fn complete_koi_todo(&self, id: &str, result_message_id: Option<i64>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE koi_todos SET status = 'done', blocked_reason = NULL, result_message_id = COALESCE(?2, result_message_id), \
             integration_status = CASE WHEN git_branch IS NOT NULL AND TRIM(git_branch) != '' AND integration_status NOT IN ('merged', 'conflict') THEN 'ready' ELSE integration_status END, \
             updated_at = ?3 WHERE id = ?1",
            params![id, result_message_id, now],
        )?;
        Ok(())
    }

    pub fn delete_koi_todo(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM koi_todos WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn delete_todos_by_pool(&self, pool_session_id: &str) -> Result<u32> {
        let count = self.conn.execute(
            "DELETE FROM koi_todos WHERE pool_session_id = ?1",
            params![pool_session_id],
        )?;
        Ok(count as u32)
    }

    // ------------------------------------------------------------------
    // Pool Sessions & Messages (Chat Pool)
    // ------------------------------------------------------------------

    pub fn create_pool_session(
        &self,
        name: &str,
        task_timeout_secs: u32,
    ) -> Result<crate::agent_core::models::PoolSession> {
        self.create_pool_session_with_dir(name, None, task_timeout_secs)
    }

    pub fn create_pool_session_with_dir(
        &self,
        name: &str,
        project_dir: Option<&str>,
        task_timeout_secs: u32,
    ) -> Result<crate::agent_core::models::PoolSession> {
        self.create_pool_session_with_meta(name, project_dir, task_timeout_secs, None, None)
    }

    pub fn create_pool_session_with_meta(
        &self,
        name: &str,
        project_dir: Option<&str>,
        task_timeout_secs: u32,
        team_id: Option<&str>,
        workflow_run_id: Option<&str>,
    ) -> Result<crate::agent_core::models::PoolSession> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO pool_sessions (id, name, org_spec, status, project_dir, task_timeout_secs, last_active_at, created_at, updated_at, team_id, workflow_run_id) \
             VALUES (?1, ?2, '', 'active', ?3, ?4, ?5, ?5, ?5, ?6, ?7)",
            params![
                id,
                name,
                project_dir,
                task_timeout_secs,
                now_str,
                team_id.filter(|s| !s.trim().is_empty()),
                workflow_run_id.filter(|s| !s.trim().is_empty()),
            ],
        )?;
        Ok(crate::agent_core::models::PoolSession {
            id,
            name: name.to_string(),
            org_spec: String::new(),
            status: "active".to_string(),
            project_dir: project_dir.map(String::from),
            task_timeout_secs,
            origin_im_binding_key: None,
            member_koi_ids: Vec::new(),
            team_id: team_id.filter(|s| !s.trim().is_empty()).map(String::from),
            workflow_run_id: workflow_run_id
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
            last_active_at: Some(now),
            created_at: now,
            updated_at: now,
        })
    }

    fn map_pool_session(
        r: &rusqlite::Row,
    ) -> rusqlite::Result<crate::agent_core::models::PoolSession> {
        Ok(crate::agent_core::models::PoolSession {
            id: r.get(0)?,
            name: r.get(1)?,
            org_spec: r.get::<_, String>(2).unwrap_or_default(),
            status: r
                .get::<_, String>(3)
                .unwrap_or_else(|_| "active".to_string()),
            project_dir: r.get::<_, Option<String>>(4)?,
            task_timeout_secs: r.get::<_, u32>(5).unwrap_or(0),
            origin_im_binding_key: r.get::<_, Option<String>>(6).ok().flatten(),
            // Hydrated separately by list/get callers; see hydrate_members.
            member_koi_ids: Vec::new(),
            team_id: r.get::<_, Option<String>>(10).ok().flatten(),
            workflow_run_id: r.get::<_, Option<String>>(11).ok().flatten(),
            last_active_at: r
                .get::<_, Option<String>>(7)?
                .and_then(|s| s.parse::<DateTime<Utc>>().ok()),
            created_at: r
                .get::<_, String>(8)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: r
                .get::<_, String>(9)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Fill in `member_koi_ids` for a freshly-mapped pool session.
    fn hydrate_members(
        &self,
        mut session: crate::agent_core::models::PoolSession,
    ) -> Result<crate::agent_core::models::PoolSession> {
        session.member_koi_ids = self.list_pool_member_ids(&session.id)?;
        Ok(session)
    }

    pub fn list_pool_sessions(&self) -> Result<Vec<crate::agent_core::models::PoolSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, org_spec, status, project_dir, task_timeout_secs, origin_im_binding_key, last_active_at, created_at, updated_at, team_id, workflow_run_id \
             FROM pool_sessions ORDER BY updated_at DESC"
        )?;
        let rows = stmt.query_map([], Self::map_pool_session)?;
        let sessions = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        sessions
            .into_iter()
            .map(|s| self.hydrate_members(s))
            .collect()
    }

    pub fn get_pool_session(
        &self,
        id: &str,
    ) -> Result<Option<crate::agent_core::models::PoolSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, org_spec, status, project_dir, task_timeout_secs, origin_im_binding_key, last_active_at, created_at, updated_at, team_id, workflow_run_id \
             FROM pool_sessions WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], Self::map_pool_session)?;
        match rows.next().transpose()? {
            Some(session) => Ok(Some(self.hydrate_members(session)?)),
            None => Ok(None),
        }
    }

    pub fn get_pool_session_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Option<crate::agent_core::models::PoolSession>> {
        if prefix.trim().is_empty() {
            return Ok(None);
        }
        if let Some(session) = self.get_pool_session(prefix)? {
            return Ok(Some(session));
        }
        let like = format!("{}%", prefix);
        let mut stmt = self.conn.prepare(
            "SELECT id, name, org_spec, status, project_dir, task_timeout_secs, origin_im_binding_key, last_active_at, created_at, updated_at, team_id, workflow_run_id \
             FROM pool_sessions WHERE id LIKE ?1 ORDER BY updated_at DESC"
        )?;
        let rows = stmt.query_map(params![like], Self::map_pool_session)?;
        let matches = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.len() {
            0 => Ok(None),
            1 => Ok(Some(
                self.hydrate_members(matches.into_iter().next().unwrap())?,
            )),
            _ => Err(anyhow::anyhow!("Pool id prefix '{}' is ambiguous", prefix)),
        }
    }

    pub fn resolve_pool_session_identifier(
        &self,
        value: &str,
    ) -> Result<Option<crate::agent_core::models::PoolSession>> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(None);
        }
        if let Some(session) = self.get_pool_session_by_prefix(value)? {
            return Ok(Some(session));
        }
        let matches: Vec<crate::agent_core::models::PoolSession> = self
            .list_pool_sessions()?
            .into_iter()
            .filter(|session| session.name == value)
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.into_iter().next()),
            _ => Err(anyhow::anyhow!("Pool identifier '{}' is ambiguous", value)),
        }
    }

    /// Find a non-archived pool for a team within a project directory.
    /// Prefers explicit `team_id` match; falls back to legacy rows with NULL
    /// `team_id` matched by pool name (pre-migration compat).
    pub fn find_active_pool(
        &self,
        project_dir: &str,
        team_id: &str,
        team_name: &str,
    ) -> Result<Option<crate::agent_core::models::PoolSession>> {
        let dir = project_dir.trim();
        let tid = team_id.trim();
        let name = team_name.trim();
        if dir.is_empty() || tid.is_empty() {
            return Ok(None);
        }
        let pools: Vec<_> = self
            .list_pool_sessions()?
            .into_iter()
            .filter(|p| {
                p.status != "archived"
                    && p.workflow_run_id.is_none()
                    && p.project_dir
                        .as_deref()
                        .map(|d| d.trim() == dir)
                        .unwrap_or(false)
            })
            .collect();
        if let Some(p) = pools
            .iter()
            .find(|p| p.team_id.as_deref() == Some(tid))
            .cloned()
        {
            return Ok(Some(p));
        }
        // Legacy: team_id unset but name matches the team template.
        if !name.is_empty() {
            if let Some(p) = pools
                .iter()
                .find(|p| p.team_id.is_none() && p.name == name)
                .cloned()
            {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    pub fn normalize_identifier_references(&self) -> Result<u32> {
        let mut updated = 0u32;

        {
            let mut stmt = self
                .conn
                .prepare("SELECT id, owner_id, claimed_by, pool_session_id FROM koi_todos")?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;

            for row in rows {
                let (todo_id, owner_id, claimed_by, pool_session_id) = row;

                if let Some(koi) = self.resolve_koi_identifier(&owner_id)? {
                    if koi.id != owner_id {
                        self.conn.execute(
                            "UPDATE koi_todos SET owner_id = ?2 WHERE id = ?1",
                            params![todo_id, koi.id],
                        )?;
                        updated += 1;
                    }
                }

                if let Some(claimed_by) = claimed_by {
                    if let Some(koi) = self.resolve_koi_identifier(&claimed_by)? {
                        if koi.id != claimed_by {
                            self.conn.execute(
                                "UPDATE koi_todos SET claimed_by = ?2 WHERE id = ?1",
                                params![todo_id, koi.id],
                            )?;
                            updated += 1;
                        }
                    }
                }

                if let Some(pool_session_id) = pool_session_id {
                    if let Some(session) = self.resolve_pool_session_identifier(&pool_session_id)? {
                        if session.id != pool_session_id {
                            self.conn.execute(
                                "UPDATE koi_todos SET pool_session_id = ?2 WHERE id = ?1",
                                params![todo_id, session.id],
                            )?;
                            updated += 1;
                        }
                    }
                }
            }
        }

        {
            let mut stmt = self.conn.prepare(
                "SELECT id, owner_id, scope_type, scope_id, project_scope_id FROM memories",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;

            for row in rows {
                let (memory_id, owner_id, scope_type, scope_id, project_scope_id) = row;

                if let Some(koi) = self.resolve_koi_identifier(&owner_id)? {
                    if koi.id != owner_id {
                        self.conn.execute(
                            "UPDATE memories SET owner_id = ?2 WHERE id = ?1",
                            params![memory_id, koi.id],
                        )?;
                        updated += 1;
                    }

                    if scope_type == "private" && scope_id != koi.id {
                        self.conn.execute(
                            "UPDATE memories SET scope_id = ?2 WHERE id = ?1",
                            params![memory_id, koi.id],
                        )?;
                        updated += 1;
                    }
                }

                if scope_type == "project" {
                    if let Some(session) = self.resolve_pool_session_identifier(&scope_id)? {
                        if session.id != scope_id {
                            self.conn.execute(
                                "UPDATE memories SET scope_id = ?2 WHERE id = ?1",
                                params![memory_id, session.id],
                            )?;
                            updated += 1;
                        }
                    }
                }

                if let Some(project_scope_id) = project_scope_id {
                    if let Some(session) =
                        self.resolve_pool_session_identifier(&project_scope_id)?
                    {
                        if session.id != project_scope_id {
                            self.conn.execute(
                                "UPDATE memories SET project_scope_id = ?2 WHERE id = ?1",
                                params![memory_id, session.id],
                            )?;
                            updated += 1;
                        }
                    }
                }
            }
        }

        Ok(updated)
    }

    /// Return all todos for a pool that are in active states (todo / in_progress / blocked).
    pub fn list_active_todos_by_pool(
        &self,
        pool_session_id: &str,
    ) -> Result<Vec<crate::agent_core::models::KoiTodo>> {
        let sql = format!(
            "SELECT {} FROM koi_todos WHERE pool_session_id = ?1 AND status IN ('todo','in_progress','blocked') ORDER BY created_at DESC",
            Self::KOI_TODO_COLS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![pool_session_id], Self::map_koi_todo)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn update_pool_session_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status, now],
        )?;
        Ok(())
    }

    pub fn touch_pool_session_active(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET last_active_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn find_related_pool_sessions(
        &self,
        keywords: &str,
    ) -> Result<Vec<crate::agent_core::models::PoolSession>> {
        let kw_lower = keywords.to_lowercase();
        let terms: Vec<&str> = kw_lower.split_whitespace().collect();
        if terms.is_empty() {
            return self.list_pool_sessions();
        }
        let all = self.list_pool_sessions()?;
        let mut results: Vec<(usize, crate::agent_core::models::PoolSession)> = Vec::new();
        for session in all {
            let haystack = format!(
                "{} {} {}",
                session.name.to_lowercase(),
                session.org_spec.to_lowercase(),
                session.status,
            );
            let score = terms.iter().filter(|t| haystack.contains(*t)).count();
            if score > 0 {
                results.push((score, session));
            }
        }
        results.sort_by_key(|r| std::cmp::Reverse(r.0));
        Ok(results.into_iter().map(|(_, s)| s).collect())
    }

    pub fn update_pool_org_spec(&self, id: &str, org_spec: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET org_spec = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, org_spec, now],
        )?;
        Ok(())
    }

    pub fn update_pool_session_name(&self, id: &str, name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now],
        )?;
        Ok(())
    }

    pub fn update_pool_session_config(
        &self,
        id: &str,
        task_timeout_secs: Option<u32>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET task_timeout_secs = COALESCE(?2, task_timeout_secs), updated_at = ?3 WHERE id = ?1",
            params![id, task_timeout_secs, now],
        )?;
        Ok(())
    }

    pub fn update_pool_session_dir(&self, id: &str, project_dir: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE pool_sessions SET project_dir = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, project_dir, now],
        )?;
        Ok(())
    }

    pub fn delete_pool_session(&self, id: &str) -> Result<()> {
        let internal_session_ids = self.internal_session_ids_for_pool(id)?;
        for session_id in &internal_session_ids {
            self.delete_session(session_id)?;
        }
        self.conn.execute(
            "DELETE FROM koi_todos WHERE pool_session_id = ?1",
            params![id],
        )?;
        self.conn.execute(
            "DELETE FROM pool_messages WHERE pool_session_id = ?1",
            params![id],
        )?;
        self.conn
            .execute("DELETE FROM pool_sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ─── app_meta (one-shot migration guards) ──────────────────────────

    /// True if the given meta flag has been set previously.
    pub fn meta_flag_is_set(&self, key: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM app_meta WHERE key = ?1",
                params![key],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Persist a meta flag so a one-time migration does not run again.
    pub fn set_meta_flag(&self, key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO app_meta (key, value) VALUES (?1, '1')",
            params![key],
        )?;
        Ok(())
    }

    // ─── pool membership (project team roster) ─────────────────────────

    /// One-shot backfill: seed each pool's roster from the distinct Koi
    /// owners that already have todos in it. Idempotent via the table's
    /// primary key. Pools without historical Koi todos stay empty.
    pub fn backfill_pool_members_from_todos(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            INSERT OR IGNORE INTO pool_members (pool_session_id, koi_id, joined_at)
            SELECT DISTINCT t.pool_session_id, t.owner_id,
                   strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            FROM koi_todos t
            WHERE t.pool_session_id IS NOT NULL
              AND t.owner_id IN (SELECT id FROM kois);
            ",
        )?;
        Ok(())
    }

    /// Ids of all Koi that are members of the given pool.
    pub fn list_pool_member_ids(&self, pool_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT koi_id FROM pool_members WHERE pool_session_id = ?1 ORDER BY joined_at ASC",
        )?;
        let rows = stmt.query_map(params![pool_id], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Full Koi definitions for every member of the given pool, joined
    /// against the `kois` table and ordered by join time.
    pub fn list_pool_members(
        &self,
        pool_id: &str,
    ) -> Result<Vec<crate::agent_core::models::KoiDefinition>> {
        let mut stmt = self.conn.prepare(
            "SELECT k.id, k.name, k.role, k.icon, k.color, k.system_prompt, k.description, k.status, \
                    k.created_at, k.updated_at, k.llm_provider_id, k.max_iterations, k.task_timeout_secs \
             FROM pool_members m JOIN kois k ON k.id = m.koi_id \
             WHERE m.pool_session_id = ?1 ORDER BY m.joined_at ASC",
        )?;
        let rows = stmt.query_map(params![pool_id], |r| {
            Ok(crate::agent_core::models::KoiDefinition {
                id: r.get(0)?,
                name: r.get(1)?,
                role: r.get::<_, String>(2).unwrap_or_default(),
                icon: r.get(3)?,
                color: r.get(4)?,
                system_prompt: r.get(5)?,
                description: r.get(6)?,
                status: r.get(7)?,
                created_at: r
                    .get::<_, String>(8)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
                llm_provider_id: r.get(10)?,
                max_iterations: r.get::<_, u32>(11).unwrap_or(0),
                task_timeout_secs: r.get::<_, u32>(12).unwrap_or(0),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// True if the Koi is a member of the given pool.
    pub fn is_pool_member(&self, pool_id: &str, koi_id: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pool_members WHERE pool_session_id = ?1 AND koi_id = ?2",
            params![pool_id, koi_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// Add a Koi to a pool's roster (idempotent).
    pub fn add_pool_member(&self, pool_id: &str, koi_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO pool_members (pool_session_id, koi_id, joined_at) \
             VALUES (?1, ?2, ?3)",
            params![pool_id, koi_id, now],
        )?;
        Ok(())
    }

    /// Remove a Koi from a pool's roster.
    pub fn remove_pool_member(&self, pool_id: &str, koi_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM pool_members WHERE pool_session_id = ?1 AND koi_id = ?2",
            params![pool_id, koi_id],
        )?;
        Ok(())
    }

    /// Count active (not done/cancelled) todos a member owns in a pool —
    /// used to block removal of a Koi that still has live work.
    pub fn count_active_todos_for_member(&self, pool_id: &str, koi_id: &str) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM koi_todos \
             WHERE pool_session_id = ?1 AND owner_id = ?2 \
               AND status NOT IN ('done', 'cancelled')",
            params![pool_id, koi_id],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    fn internal_session_ids_for_pool(&self, pool_id: &str) -> Result<Vec<String>> {
        let suffix = format!("_{}", pool_id);
        let coordinator_id = format!("xiaoheng_pool_{}", pool_id);
        let legacy_coordinator_id = format!("piscis_pool_{}", pool_id);
        let mut stmt = self.conn.prepare(
            "SELECT id, COALESCE(source, 'chat') FROM sessions
             WHERE id IN (?1, ?2)
                OR source IN ('codex_staffdeck', 'piscis_pool', 'piscis_heartbeat_pool')
                OR id LIKE 'koi\\_%' ESCAPE '\\'",
        )?;
        let rows = stmt.query_map(params![coordinator_id, legacy_coordinator_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut ids = Vec::new();
        for row in rows {
            let (session_id, source) = row?;
            let linked_by_id = session_id == coordinator_id
                || session_id == legacy_coordinator_id
                || (session_id.ends_with(&suffix)
                    && (session_id.starts_with("koi_runtime_")
                        || session_id.starts_with("koi_notify_")
                        || session_id.starts_with("koi_")));
            let linked_by_source = matches!(
                source.as_str(),
                "codex_staffdeck" | "piscis_pool" | "piscis_heartbeat_pool"
            ) && session_id.ends_with(&suffix);
            if linked_by_id || linked_by_source {
                ids.push(session_id);
            }
        }
        Ok(ids)
    }

    /// Persist (or clear) the `binding_key` of the IM conversation
    /// that originally requested this pool. Used by the IM ↔ pool
    /// fan-out path so heartbeat alerts can reach the same chat that
    /// kicked off the work, even after the desktop is restarted.
    ///
    /// Passing `None` clears the link.
    pub fn set_pool_origin_im_binding(
        &self,
        pool_id: &str,
        binding_key: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE pool_sessions SET origin_im_binding_key = ?1, updated_at = ?2 WHERE id = ?3",
            params![binding_key, Utc::now().to_rfc3339(), pool_id],
        )?;
        Ok(())
    }

    /// Channel-agnostic lookup: resolve the most recent IM binding
    /// associated with the given Piscis `session_id`. Useful when the
    /// caller (e.g. the pool tool) only knows the session and wants
    /// to discover whether the conversation is rooted in IM.
    pub fn find_im_session_binding_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<ImSessionBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT binding_key, channel, external_conversation_key, session_id, peer_id,
                    peer_name, is_group, group_name, latest_reply_target, routing_state_json,
                    created_at, updated_at, last_inbound_at
             FROM im_session_bindings
             WHERE session_id = ?1
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![session_id], map_im_session_binding_row)?;
        Ok(rows.next().transpose()?)
    }

    /// Resolve the IM binding that originally created a pool, if one
    /// was recorded on `pool_sessions.origin_im_binding_key`.
    pub fn find_im_session_binding_for_pool(
        &self,
        pool_id: &str,
    ) -> Result<Option<ImSessionBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.binding_key, b.channel, b.external_conversation_key, b.session_id, b.peer_id,
                    b.peer_name, b.is_group, b.group_name, b.latest_reply_target, b.routing_state_json,
                    b.created_at, b.updated_at, b.last_inbound_at
             FROM pool_sessions p
             JOIN im_session_bindings b ON b.binding_key = p.origin_im_binding_key
             WHERE p.id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![pool_id], map_im_session_binding_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_scheduled_task_notify_targets(
        &self,
        task_id: &str,
        notify_targets_json: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE scheduled_tasks SET notify_targets_json = ?1 WHERE id = ?2",
            params![notify_targets_json, task_id],
        )?;
        Ok(())
    }

    pub fn get_scheduled_task_notify_targets(&self, task_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT notify_targets_json FROM scheduled_tasks WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![task_id], |r| r.get::<_, Option<String>>(0))?;
        Ok(rows.next().transpose()?.flatten())
    }

    pub fn insert_pool_message(
        &self,
        pool_session_id: &str,
        sender_id: &str,
        content: &str,
        msg_type: &str,
        metadata: &str,
    ) -> Result<crate::agent_core::models::PoolMessage> {
        self.insert_pool_message_ext(
            pool_session_id,
            sender_id,
            content,
            msg_type,
            metadata,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_pool_message_ext(
        &self,
        pool_session_id: &str,
        sender_id: &str,
        content: &str,
        msg_type: &str,
        metadata: &str,
        todo_id: Option<&str>,
        reply_to_message_id: Option<i64>,
        event_type: Option<&str>,
    ) -> Result<crate::agent_core::models::PoolMessage> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        self.conn.execute(
            "INSERT INTO pool_messages (pool_session_id, sender_id, content, msg_type, metadata, todo_id, reply_to_message_id, event_type, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![pool_session_id, sender_id, content, msg_type, metadata, todo_id, reply_to_message_id, event_type, now_str],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "UPDATE pool_sessions SET updated_at = ?1, last_active_at = ?1 WHERE id = ?2",
            params![now_str, pool_session_id],
        )?;
        Ok(crate::agent_core::models::PoolMessage {
            id,
            pool_session_id: pool_session_id.to_string(),
            sender_id: sender_id.to_string(),
            content: content.to_string(),
            msg_type: msg_type.to_string(),
            metadata: metadata.to_string(),
            todo_id: todo_id.map(String::from),
            reply_to_message_id,
            event_type: event_type.map(String::from),
            created_at: now,
        })
    }

    pub fn get_pool_messages(
        &self,
        pool_session_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<crate::agent_core::models::PoolMessage>> {
        // Fetch the newest `limit` rows starting at `offset` from the end,
        // then re-sort ascending so callers always receive chronological order.
        let mut stmt = self.conn.prepare(
            "SELECT id, pool_session_id, sender_id, content, msg_type, metadata, \
             todo_id, reply_to_message_id, event_type, created_at \
             FROM ( \
               SELECT id, pool_session_id, sender_id, content, msg_type, metadata, \
                      todo_id, reply_to_message_id, event_type, created_at \
               FROM pool_messages WHERE pool_session_id = ?1 \
               ORDER BY created_at DESC LIMIT ?2 OFFSET ?3 \
             ) ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(
            params![pool_session_id, limit, offset],
            Self::map_pool_message,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Get the content of the most recent "result" type pool message sent by a specific Koi.
    /// Used to recover the complete_todo summary when the agent loop ends with a tool call.
    pub fn get_latest_result_message(
        &self,
        pool_session_id: &str,
        sender_id: &str,
    ) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT content FROM pool_messages \
             WHERE pool_session_id = ?1 AND sender_id = ?2 AND msg_type = 'result' \
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![pool_session_id, sender_id], |row| {
            row.get::<_, String>(0)
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Get the id of the most recent "result" message from a Koi that is not yet linked to a todo.
    /// Used by runtime to link a complete_todo-written message to the current todo.
    pub fn get_latest_unlinked_result_message_id(
        &self,
        pool_session_id: &str,
        sender_id: &str,
    ) -> Result<Option<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM pool_messages \
             WHERE pool_session_id = ?1 AND sender_id = ?2 AND msg_type = 'result' \
             AND (todo_id IS NULL OR todo_id = '') \
             ORDER BY created_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![pool_session_id, sender_id], |row| {
            row.get::<_, i64>(0)
        })?;
        Ok(rows.next().transpose()?)
    }

    /// Link a pool message to a todo (set its todo_id field).
    pub fn link_pool_message_to_todo(&self, message_id: i64, todo_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE pool_messages SET todo_id = ?2 WHERE id = ?1",
            params![message_id, todo_id],
        )?;
        Ok(())
    }

    pub fn get_pool_message_by_id(
        &self,
        id: i64,
    ) -> Result<Option<crate::agent_core::models::PoolMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pool_session_id, sender_id, content, msg_type, metadata, \
             todo_id, reply_to_message_id, event_type, created_at \
             FROM pool_messages WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], Self::map_pool_message)?;
        Ok(rows.next().transpose()?)
    }

    /// Get pool messages linked to a specific todo
    pub fn get_pool_messages_for_todo(
        &self,
        todo_id: &str,
    ) -> Result<Vec<crate::agent_core::models::PoolMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pool_session_id, sender_id, content, msg_type, metadata, \
             todo_id, reply_to_message_id, event_type, created_at \
             FROM pool_messages WHERE todo_id = ?1 \
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![todo_id], Self::map_pool_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn map_pool_message(
        r: &rusqlite::Row,
    ) -> rusqlite::Result<crate::agent_core::models::PoolMessage> {
        Ok(crate::agent_core::models::PoolMessage {
            id: r.get(0)?,
            pool_session_id: r.get(1)?,
            sender_id: r.get(2)?,
            content: r.get(3)?,
            msg_type: r.get(4)?,
            metadata: r.get(5)?,
            todo_id: r.get(6)?,
            reply_to_message_id: r.get(7)?,
            event_type: r.get(8)?,
            created_at: r
                .get::<_, String>(9)?
                .parse::<DateTime<Utc>>()
                .unwrap_or_else(|_| Utc::now()),
        })
    }

    /// Reset all "busy" Koi back to "idle" — called on startup to fix stale state from crashes.
    pub fn recover_stale_koi_status(&self) -> Result<u32> {
        let count = self.conn.execute(
            "UPDATE kois SET status = 'idle', updated_at = datetime('now') WHERE status = 'busy'",
            [],
        )?;
        if count > 0 {
            tracing::info!("Startup recovery: reset {} stale busy Koi to idle", count);
        }
        Ok(count as u32)
    }

    /// Reset all "in_progress" todos back to "todo" — called on startup.
    /// These were being worked on when the app crashed.
    pub fn recover_stale_todos(&self) -> Result<u32> {
        let count = self.conn.execute(
            "UPDATE koi_todos SET status = 'todo', claimed_by = NULL, claimed_at = NULL, updated_at = datetime('now') WHERE status = 'in_progress'",
            [],
        )?;
        if count > 0 {
            tracing::info!(
                "Startup recovery: reset {} stale in_progress todos to todo",
                count
            );
        }
        Ok(count as u32)
    }

    /// Reset Koi that have been "busy" longer than max_age_secs — for runtime watchdog.
    pub fn recover_stale_busy_kois(&self, max_age_secs: i64) -> Result<u32> {
        let count = if max_age_secs <= 0 {
            self.conn.execute(
                "UPDATE kois SET status = 'idle', updated_at = datetime('now') WHERE status = 'busy'",
                [],
            )?
        } else {
            self.conn.execute(
                "UPDATE kois SET status = 'idle', updated_at = datetime('now') WHERE status = 'busy' AND updated_at < datetime('now', ?)",
                rusqlite::params![format!("-{} seconds", max_age_secs)],
            )?
        };
        Ok(count as u32)
    }

    /// Reset "in_progress" todos older than max_age_secs — for runtime watchdog.
    pub fn recover_stale_in_progress_todos(&self, max_age_secs: i64) -> Result<u32> {
        let count = if max_age_secs <= 0 {
            self.conn.execute(
                "UPDATE koi_todos SET status = 'todo', claimed_by = NULL, claimed_at = NULL, updated_at = datetime('now') WHERE status = 'in_progress'",
                [],
            )?
        } else {
            self.conn.execute(
                "UPDATE koi_todos SET status = 'todo', claimed_by = NULL, claimed_at = NULL, updated_at = datetime('now') WHERE status = 'in_progress' AND updated_at < datetime('now', ?)",
                rusqlite::params![format!("-{} seconds", max_age_secs)],
            )?
        };
        Ok(count as u32)
    }

    pub fn recover_stale_running_sessions(&self, max_age_secs: i64) -> Result<u32> {
        let count = if max_age_secs <= 0 {
            self.conn.execute(
                "UPDATE sessions SET status = 'idle', updated_at = datetime('now') WHERE status = 'running'",
                [],
            )?
        } else {
            self.conn.execute(
                "UPDATE sessions SET status = 'idle', updated_at = datetime('now') WHERE status = 'running' AND updated_at < datetime('now', ?)",
                rusqlite::params![format!("-{} seconds", max_age_secs)],
            )?
        };
        Ok(count as u32)
    }

    fn row_to_skill_usage(r: &rusqlite::Row<'_>) -> rusqlite::Result<SkillUsage> {
        Ok(SkillUsage {
            skill_id: r.get(0)?,
            view_count: r.get(1)?,
            use_count: r.get(2)?,
            patch_count: r.get(3)?,
            last_used_at: r.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
            last_patched_at: r.get::<_, Option<String>>(5)?.and_then(|s| s.parse().ok()),
            state: r.get(6)?,
            pinned: r.get::<_, i64>(7)? != 0,
            created_by: r.get(8)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_disallowed_koi_name_char, normalize_koi_name, Database, ImSessionBindingUpsert,
    };

    #[test]
    fn deleted_memory_exports_a_tombstone_and_cannot_be_restored_from_older_cloud_data() {
        let db = Database::open_in_memory().unwrap();
        let memory = db.save_memory("用户喜欢简洁中文", "preference", 0.9, None, "xiaoheng", "private", "xiaoheng", None).unwrap();
        let old = db.export_memory_changes().unwrap();
        db.delete_memory(&memory.id).unwrap();
        let deleted = db.export_memory_changes().unwrap();
        assert_eq!(deleted.len(), 1);
        assert!(deleted[0].memory.is_none());
        db.apply_memory_changes(&old).unwrap();
        assert!(db.list_memories().unwrap().is_empty());

        let other = Database::open_in_memory().unwrap();
        other.apply_memory_changes(&old).unwrap();
        assert_eq!(other.list_memories().unwrap().len(), 1);
        other.apply_memory_changes(&deleted).unwrap();
        assert!(other.list_memories().unwrap().is_empty());
    }

    #[test]
    fn local_memory_sync_rejects_a_second_account() {
        let db = Database::open_in_memory().unwrap();
        db.ensure_memory_sync_account("alice").unwrap();
        db.ensure_memory_sync_account("alice").unwrap();
        assert!(db.ensure_memory_sync_account("bob").is_err());
    }

    #[test]
    fn cloud_restore_preserves_project_scope_without_making_it_global() {
        let source = Database::open_in_memory().unwrap();
        let memory = source.save_memory("仅本项目使用的偏好", "preference", 0.9, None,
            "xiaoheng", "private", "xiaoheng", Some("project-123")).unwrap();
        let target = Database::open_in_memory().unwrap();
        target.apply_memory_changes(&source.export_memory_changes().unwrap()).unwrap();
        let restored = target.list_memories().unwrap().into_iter()
            .find(|item| item.id == memory.id).unwrap();
        assert_eq!(restored.project_scope_id.as_deref(), Some("project-123"));
    }

    #[test]
    fn cloud_sync_carries_reobserved_memory_confidence() {
        let source = Database::open_in_memory().unwrap();
        let target = Database::open_in_memory().unwrap();
        let first = source.save_memory("用户偏好简洁中文", "preference", 0.5, None,
            "xiaoheng", "private", "xiaoheng", None).unwrap();
        let original = source.export_memory_changes().unwrap();
        target.apply_memory_changes(&original).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(2));
        let observed = source.save_memory("用户偏好简洁中文", "preference", 0.5, None,
            "xiaoheng", "private", "xiaoheng", None).unwrap();
        assert_eq!(observed.id, first.id);
        assert_eq!(observed.updated_at, first.updated_at);
        assert!(observed.confidence > first.confidence);

        let changes = source.export_memory_changes().unwrap();
        assert!(changes[0].updated_at > original[0].updated_at);
        target.apply_memory_changes(&changes).unwrap();
        let restored = target.list_memories().unwrap().into_iter()
            .find(|item| item.id == first.id).unwrap();
        assert_eq!(restored.confidence, observed.confidence);
        assert_eq!(restored.last_seen_at, observed.last_seen_at);
        assert_eq!(restored.updated_at, first.updated_at);
    }

    #[test]
    fn mobile_message_import_preserves_ids_and_is_idempotent() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = "3ecc3126-67a5-4e33-bfad-f1fe2308ef9e";
        db.ensure_fixed_session(session, "From phone", "mobile").unwrap();
        let message = "11111111-1111-4111-8111-111111111111";
        assert!(db.import_mobile_message(session, message, "user", "hello").unwrap());
        assert!(!db.import_mobile_message(session, message, "user", "hello").unwrap());
        let imported = db.get_messages(session, 10, 0).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].id, message);
        assert_eq!(db.get_session(session).unwrap().unwrap().message_count, 1);
    }

    #[test]
    fn shared_messages_preserve_distinct_ids_and_server_sequence() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = "3ecc3126-67a5-4e33-bfad-f1fe2308ef9e";
        db.ensure_fixed_session(session, "From phone", "mobile").unwrap();
        let a = "11111111-1111-4111-8111-111111111111";
        let b = "22222222-2222-4222-8222-222222222222";
        assert!(db.import_shared_message(session, b, "assistant", "same", 22, "desktop").unwrap());
        assert!(db.import_shared_message(session, a, "assistant", "same", 21, "mobile").unwrap());
        assert!(!db.import_shared_message(session, a, "assistant", "same", 21, "mobile").unwrap());
        let messages = db.get_messages(session, 10, 0).unwrap();
        assert_eq!(messages.iter().map(|message| message.id.as_str()).collect::<Vec<_>>(), vec![a, b]);
    }

    #[test]
    fn mobile_executor_session_is_hidden_from_regular_session_list() {
        let db = Database::open_in_memory().unwrap();
        db.ensure_fixed_session("visible", "Phone chat", "mobile").unwrap();
        db.ensure_fixed_session("internal", "Desktop job", "mobile_executor").unwrap();
        let sessions = db.list_sessions(20, 0, true).unwrap();
        assert!(sessions.iter().any(|session| session.id == "visible"));
        assert!(!sessions.iter().any(|session| session.id == "internal"));
        let signed_out = db.list_sessions(20, 0, false).unwrap();
        assert!(!signed_out.iter().any(|session| session.id == "visible"));
    }

    #[test]
    fn clearing_mobile_cache_does_not_delete_desktop_chats() {
        let db = Database::open_in_memory().unwrap();
        db.ensure_fixed_session("phone", "Phone chat", "mobile").unwrap();
        db.ensure_fixed_session("desktop", "Desktop chat", "chat").unwrap();
        assert_eq!(db.delete_all_mobile_sessions().unwrap(), 1);
        assert!(db.get_session("phone").unwrap().is_none());
        assert!(db.get_session("desktop").unwrap().is_some());
    }

    #[test]
    fn desktop_shared_turn_finishes_only_after_assistant_is_uploaded() {
        let db = Database::open_in_memory().unwrap();
        let session = "3ecc3126-67a5-4e33-bfad-f1fe2308ef9e";
        let user = "11111111-1111-4111-8111-111111111111";
        let turn = "22222222-2222-4222-8222-222222222222";
        db.ensure_fixed_session(session, "Phone chat", "mobile").unwrap();
        db.import_shared_message(session, user, "user", "do work", 41, "desktop").unwrap();
        db.record_shared_desktop_turn(session, turn, user).unwrap();
        assert_eq!(db.pending_shared_desktop_turns().unwrap().len(), 1);
        assert!(!db.has_shared_assistant_after(session, user).unwrap());
        let answer = db.append_message(session, "assistant", "done").unwrap();
        assert!(!db.has_shared_assistant_after(session, user).unwrap());
        db.import_shared_message(session, &answer.id, "assistant", "done", 42, "desktop").unwrap();
        assert!(db.has_shared_assistant_after(session, user).unwrap());
        db.complete_shared_desktop_turn(turn).unwrap();
        assert!(db.pending_shared_desktop_turns().unwrap().is_empty());
    }

    #[test]
    fn execution_events_and_message_context_roundtrip() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = db.create_session(Some("Execution history")).expect("session");
        let context = r#"[{"targetType":"artifact","artifactId":"report-1","comment":"标题更简洁"}]"#;

        let message = db
            .append_message_with_context(&session.id, "user", "请按批注修改", Some(context))
            .expect("message with context");
        db.append_turn_execution_event(
            &session.id,
            "turn-1",
            1,
            "reasoning_summary",
            r#"{"text":"正在核对素材"}"#,
        )
        .expect("execution event");
        db.append_turn_execution_event(
            &session.id,
            "turn-1",
            2,
            "done",
            r#"{"total_input_tokens":12,"total_output_tokens":4}"#,
        )
        .expect("terminal event");

        assert_eq!(message.content, "请按批注修改");
        assert_eq!(message.context_json.as_deref(), Some(context));
        let messages = db.get_messages(&session.id, 20, 0).expect("messages");
        assert_eq!(messages[0].context_json.as_deref(), Some(context));

        let events = db
            .list_turn_execution_events(&session.id, 20)
            .expect("execution history");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].turn_id, "turn-1");
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[0].event_type, "reasoning_summary");
        assert_eq!(events[1].sequence, 2);
    }

    #[test]
    fn execution_event_storage_enforces_payload_and_turn_limits() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = db.create_session(Some("Execution limits")).expect("session");

        assert!(db
            .append_turn_execution_event(&session.id, "invalid-json", 1, "tool_start", "not json")
            .is_err());
        let oversized = serde_json::json!({"text":"x".repeat(17 * 1024)}).to_string();
        assert!(db
            .append_turn_execution_event(&session.id, "oversized", 1, "tool_end", &oversized)
            .is_err());

        for sequence in 1..=500 {
            db.append_turn_execution_event(
                &session.id,
                "bounded-turn",
                sequence,
                "plan_update",
                r#"{"items":[]}"#,
            )
            .expect("event within limit");
        }
        assert!(db
            .append_turn_execution_event(
                &session.id,
                "bounded-turn",
                501,
                "done",
                r#"{}"#,
            )
            .is_err());
    }

    #[test]
    fn session_artifacts_roundtrip_by_session() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = db.create_session(Some("Artifacts")).expect("session");
        let other = db.create_session(Some("Other")).expect("other session");

        let artifact = db
            .add_session_artifact(
                &session.id,
                "Report",
                "file",
                Some("/tmp/report.md"),
                "Generated report",
                Some("app_control"),
                Some("tu_123"),
                Some(r#"{"kind":"markdown"}"#),
            )
            .expect("artifact");
        assert_eq!(artifact.name, "Report");

        db.add_session_artifact(
            &other.id,
            "Other",
            "file",
            Some("/tmp/other.md"),
            "Other report",
            None,
            None,
            None,
        )
        .expect("other artifact");

        let artifacts = db
            .list_session_artifacts(&session.id, 20)
            .expect("list artifacts");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].id, artifact.id);
        assert_eq!(artifacts[0].uri.as_deref(), Some("/tmp/report.md"));
    }

    #[test]
    fn session_context_state_defaults_and_updates_roundtrip() {
        let db = Database::open_in_memory().expect("in-memory db");
        let session = db.create_session(Some("Context")).expect("session");

        let initial = db
            .get_session_context_state(&session.id)
            .expect("state query")
            .expect("state exists");
        assert_eq!(initial.rolling_summary, "");
        assert_eq!(initial.rolling_summary_version, 0);
        assert_eq!(initial.total_input_tokens, 0);
        assert_eq!(initial.total_output_tokens, 0);
        assert!(initial.last_compacted_at.is_none());

        db.update_session_usage_totals(&session.id, 123, 45)
            .expect("usage update");
        db.update_session_rolling_summary(&session.id, "summary body", 2)
            .expect("summary update");

        let updated = db
            .get_session(&session.id)
            .expect("session query")
            .expect("session exists");
        assert_eq!(updated.total_input_tokens, 123);
        assert_eq!(updated.total_output_tokens, 45);
        assert_eq!(updated.rolling_summary, "summary body");
        assert_eq!(updated.rolling_summary_version, 2);
        assert!(updated.last_compacted_at.is_some());
    }

    #[test]
    fn normalize_koi_name_rejects_spaces_and_moji() {
        assert_eq!(normalize_koi_name("  Alpha  ").expect("trim ok"), "Alpha");
        assert!(normalize_koi_name("Alpha Beta").is_err());
        assert!(normalize_koi_name("Alpha🐟").is_err());
        assert!(normalize_koi_name(" ").is_err());
        assert!(is_disallowed_koi_name_char('🐟'));
        assert!(!is_disallowed_koi_name_char('测'));
    }

    #[test]
    fn create_and_update_koi_enforce_name_rules() {
        let db = Database::open_in_memory().expect("in-memory db");
        assert!(db
            .create_koi("Alpha Beta", "role", "🐟", "#000", "", "", None, 0, 0)
            .is_err());
        let koi = db
            .create_koi("Alpha", "role", "🐟", "#000", "", "", None, 0, 0)
            .expect("create koi");
        assert!(db
            .update_koi(
                &koi.id,
                Some("Beta 🐠"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None
            )
            .is_err());
    }

    #[test]
    fn im_session_binding_roundtrip_and_session_lookup_work() {
        let db = Database::open_in_memory().expect("in-memory db");
        db.ensure_im_session("im_wechat_fixed", "wechat", "im_wechat")
            .expect("session");
        let pool = db
            .create_pool_session("wechat pool", 0)
            .expect("create pool");

        let binding = db
            .upsert_im_session_binding(&ImSessionBindingUpsert {
                binding_key: "wechat::dm:wx-user-1".to_string(),
                channel: "wechat".to_string(),
                external_conversation_key: "dm:wx-user-1".to_string(),
                session_id: "im_wechat_fixed".to_string(),
                peer_id: "wx-user-1".to_string(),
                peer_name: Some("Alice".to_string()),
                is_group: false,
                group_name: None,
                latest_reply_target: "wx-user-1|ctx-1".to_string(),
                routing_state_json: Some(
                    r#"{"context_token":"ctx-1","from_user_id":"wx-user-1"}"#.to_string(),
                ),
            })
            .expect("binding");

        assert_eq!(binding.session_id, "im_wechat_fixed");
        assert_eq!(binding.latest_reply_target, "wx-user-1|ctx-1");

        let by_key = db
            .get_im_session_binding("wechat::dm:wx-user-1")
            .expect("binding by key")
            .expect("binding exists");
        assert_eq!(by_key.peer_name.as_deref(), Some("Alice"));

        let by_session = db
            .get_im_session_binding_by_session("im_wechat_fixed", "wechat")
            .expect("binding by session")
            .expect("binding exists");
        assert_eq!(by_session.binding_key, "wechat::dm:wx-user-1");

        let by_recipient = db
            .find_im_session_binding_for_channel_recipient("wechat", "wx-user-1")
            .expect("binding by peer id")
            .expect("binding exists");
        assert_eq!(by_recipient.binding_key, "wechat::dm:wx-user-1");

        let by_reply_target = db
            .find_im_session_binding_for_channel_recipient("wechat", "wx-user-1|ctx-1")
            .expect("binding by reply target")
            .expect("binding exists");
        assert_eq!(by_reply_target.binding_key, "wechat::dm:wx-user-1");

        db.set_pool_origin_im_binding(&pool.id, Some("wechat::dm:wx-user-1"))
            .expect("set pool origin binding");
        let by_pool = db
            .find_im_session_binding_for_pool(&pool.id)
            .expect("binding by pool")
            .expect("pool binding exists");
        assert_eq!(by_pool.binding_key, "wechat::dm:wx-user-1");

        let missing_pool = db
            .create_pool_session("no binding", 0)
            .expect("create pool without binding");
        assert!(db
            .find_im_session_binding_for_pool(&missing_pool.id)
            .expect("missing pool lookup")
            .is_none());
    }

    #[test]
    fn delete_pool_session_removes_linked_internal_koi_sessions() {
        let db = Database::open_in_memory().expect("in-memory db");
        let pool = db
            .create_pool_session("cleanup pool", 0)
            .expect("create pool");
        let other_pool = db
            .create_pool_session("other pool", 0)
            .expect("create other pool");

        let linked = [
            format!("xiaoheng_pool_{}", pool.id),
            format!("piscis_pool_{}", pool.id),
            format!("koi_runtime_alpha_{}", pool.id),
            format!("koi_notify_beta_{}", pool.id),
            format!("koi_gamma_{}", pool.id),
        ];
        for session_id in &linked {
            db.ensure_fixed_session(session_id, session_id, "codex_staffdeck")
                .expect("internal session");
            db.append_message(session_id, "assistant", "large hidden history")
                .expect("message");
            db.append_audit(session_id, "pool_org", "test", None, None, false)
                .expect("audit");
            db.upsert_checkpoint(session_id, 1, "[]")
                .expect("checkpoint");
        }

        let unrelated = format!("koi_runtime_alpha_{}", other_pool.id);
        db.ensure_fixed_session(&unrelated, "unrelated", "piscis_pool")
            .expect("unrelated session");

        db.delete_pool_session(&pool.id).expect("delete pool");

        for session_id in &linked {
            assert!(
                db.get_session(session_id)
                    .expect("query linked session")
                    .is_none(),
                "{session_id} should be removed with its pool"
            );
            assert!(
                db.get_messages(session_id, 10, 0)
                    .expect("query linked messages")
                    .is_empty(),
                "{session_id} messages should cascade through sessions"
            );
            assert!(
                db.get_audit_log(Some(session_id), None, 10, 0)
                    .expect("query linked audit")
                    .is_empty(),
                "{session_id} audit rows should be removed"
            );
            assert!(
                db.load_checkpoint(session_id)
                    .expect("query linked checkpoint")
                    .is_none(),
                "{session_id} checkpoints should be removed"
            );
        }

        assert!(
            db.get_session(&unrelated)
                .expect("query unrelated")
                .is_some(),
            "internal sessions for other pools must remain"
        );
    }
}
