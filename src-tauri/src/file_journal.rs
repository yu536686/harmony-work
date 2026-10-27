//! Bounded, workspace-scoped turn snapshots. Legacy snapshots remain read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_FILES: usize = 2_000;
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TURNS: usize = 20;
pub const MAX_DIFF_BYTES: usize = 128 * 1024;
const MAX_SCAN_ENTRIES: usize = 10_000;
const ABSENT: &str = "absent";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub size: u64,
    pub hash: Option<String>,
    pub binary: bool,
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewChange {
    pub path: String,
    pub kind: String,
    pub before: Option<FileSummary>,
    pub after: Option<FileSummary>,
    pub expected_after_hash: String,
    pub can_restore: bool,
    pub restored: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTurn {
    pub turn_key: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalReview {
    pub turn_key: Option<String>,
    pub status: String,
    pub capture_error: Option<String>,
    pub capture_active: bool,
    pub turns: Vec<ReviewTurn>,
    pub changes: Vec<ReviewChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalDiff {
    pub kind: String,
    pub before_text: Option<String>,
    pub after_text: Option<String>,
    pub before: Option<FileSummary>,
    pub after: Option<FileSummary>,
    pub message: Option<String>,
}

#[derive(Clone)]
struct SnapshotFile {
    summary: FileSummary,
    content: Option<Vec<u8>>,
}

type Snapshot = BTreeMap<String, SnapshotFile>;

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

#[cfg(test)]
mod review_tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        journal: FileJournal,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("journal-t6-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let journal = FileJournal::open(&root, root.join(".xiaoheng/journal.db")).unwrap();
            Self { root, journal }
        }
        fn write(&self, path: &str, bytes: &[u8]) {
            let path = self.root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        fn legacy(&self, path: &str) {
            self.journal.conn.lock().unwrap().execute(
                "INSERT INTO file_snapshots(session_id,turn_id,tool_name,rel_path,existed,before_content,applied) VALUES ('s','t','edit',?1,1,?2,1)",
                params![path, b"old".as_slice()],
            ).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn legacy_restore_without_after_hash_is_refused() {
        let f = Fixture::new();
        f.write("user.txt", b"later user edit");
        f.legacy("user.txt");
        assert!(
            f.journal.undo_turn("s", "t").is_err(),
            "legacy snapshots cannot prove current ownership"
        );
        assert_eq!(
            std::fs::read(f.root.join("user.txt")).unwrap(),
            b"later user edit"
        );
        assert_eq!(f.journal.list_changes("s", "t").unwrap().len(), 1);
    }

    #[test]
    fn legacy_traversal_cannot_restore_outside_workspace() {
        let f = Fixture::new();
        let name = format!("journal-outside-{}", uuid::Uuid::new_v4());
        let outside = f.root.parent().unwrap().join(&name);
        std::fs::write(&outside, b"outside").unwrap();
        f.legacy(&format!("../{name}"));
        let result = f.journal.undo_turn("s", "t");
        let bytes = std::fs::read(&outside).unwrap();
        std::fs::remove_file(outside).unwrap();
        assert!(result.is_err());
        assert_eq!(bytes, b"outside");
    }

    #[test]
    fn captures_add_modify_delete_and_restores_each_with_confirmation() {
        let f = Fixture::new();
        f.write("modify.txt", b"before\n");
        f.write("delete.txt", b"deleted\n");
        f.journal.capture_before("s", "turn-1").unwrap();
        f.write("modify.txt", b"after\n");
        f.write("new.txt", b"new\n");
        fs::remove_file(f.root.join("delete.txt")).unwrap();
        f.journal.capture_after("s", "turn-1").unwrap();
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.changes.len(), 3);
        for change in &review.changes {
            let diff = f.journal.diff("s", "turn-1", &change.path).unwrap();
            assert_eq!(diff.kind, "text");
            assert!(f
                .journal
                .restore_file(
                    "s",
                    "turn-1",
                    &change.path,
                    &change.expected_after_hash,
                    false
                )
                .is_err());
            f.journal
                .restore_file(
                    "s",
                    "turn-1",
                    &change.path,
                    &change.expected_after_hash,
                    true,
                )
                .unwrap();
        }
        assert_eq!(fs::read(f.root.join("modify.txt")).unwrap(), b"before\n");
        assert_eq!(fs::read(f.root.join("delete.txt")).unwrap(), b"deleted\n");
        assert!(!f.root.join("new.txt").exists());
        assert!(f
            .journal
            .review("s", None)
            .unwrap()
            .changes
            .iter()
            .all(|c| c.restored));
    }

    #[test]
    fn restore_rejects_later_edits_recreation_stale_hash_and_wrong_session() {
        let f = Fixture::new();
        f.write("a", b"old");
        f.write("deleted", b"old");
        f.journal.capture_before("s", "t").unwrap();
        f.write("a", b"agent");
        fs::remove_file(f.root.join("deleted")).unwrap();
        f.journal.capture_after("s", "t").unwrap();
        let review = f.journal.review("s", None).unwrap();
        let c = review.changes.iter().find(|c| c.path == "a").unwrap();
        assert!(f
            .journal
            .restore_file("other", "t", "a", &c.expected_after_hash, true)
            .is_err());
        assert!(f
            .journal
            .restore_file("s", "t", "a", "wrong", true)
            .is_err());
        f.write("a", b"later edit");
        assert!(f
            .journal
            .restore_file("s", "t", "a", &c.expected_after_hash, true)
            .is_err());
        f.write("deleted", b"user recreated");
        assert!(f
            .journal
            .restore_file("s", "t", "deleted", ABSENT, true)
            .is_err());
        assert_eq!(fs::read(f.root.join("a")).unwrap(), b"later edit");
        assert_eq!(fs::read(f.root.join("deleted")).unwrap(), b"user recreated");
    }

    #[test]
    fn lifecycle_is_idempotent_and_never_invents_before() {
        let f = Fixture::new();
        assert!(f.journal.capture_after("s", "unknown").is_err());
        f.write("a", b"before");
        f.journal.capture_before("s", "t").unwrap();
        f.write("a", b"after");
        f.journal.capture_before("s", "t").unwrap();
        assert_eq!(f.journal.review("s", None).unwrap().status, "capturing");
        f.journal.capture_after("s", "t").unwrap();
        f.write("a", b"later");
        f.journal.capture_after("s", "t").unwrap();
        let diff = f.journal.diff("s", "t", "a").unwrap();
        assert_eq!(diff.before_text.as_deref(), Some("before"));
        assert_eq!(diff.after_text.as_deref(), Some("after"));
        let reopened = FileJournal::open(&f.root, f.root.join(".xiaoheng/journal.db")).unwrap();
        assert_eq!(reopened.review("s", None).unwrap().changes.len(), 1);
    }

    #[test]
    fn binary_and_large_files_do_not_fabricate_text_diffs() {
        let f = Fixture::new();
        f.journal.capture_before("s", "t").unwrap();
        f.write("binary", &[0, 255, 4]);
        f.write("large", &vec![b'x'; MAX_FILE_BYTES as usize + 1]);
        f.write("long-text", &vec![b'a'; MAX_DIFF_BYTES + 1]);
        f.write(".git/internal", b"ignored");
        f.write("node_modules/module/a", b"ignored");
        f.journal.capture_after("s", "t").unwrap();
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.changes.len(), 3);
        assert!(!review.warnings.is_empty());
        assert_eq!(f.journal.diff("s", "t", "binary").unwrap().kind, "binary");
        assert_eq!(
            f.journal.diff("s", "t", "long-text").unwrap().kind,
            "too_large"
        );
        assert!(
            !review
                .changes
                .iter()
                .find(|c| c.path == "large")
                .unwrap()
                .can_restore
        );
    }

    #[test]
    fn begin_failure_is_a_durable_unavailable_turn_not_an_empty_review() {
        let f = Fixture::new();
        for n in 0..=MAX_FILES {
            f.write(&format!("many/{n}"), b"x");
        }
        assert!(f.journal.capture_before("s", "oversized").is_err());
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.status, "unavailable");
        assert_eq!(review.turn_key.as_deref(), Some("oversized"));
        assert!(review.warnings.iter().any(|s| s.contains("FILE_LIMIT")));
        assert!(review.changes.is_empty());
        assert!(f.journal.capture_after("s", "oversized").is_err());
        fs::remove_dir_all(f.root.join("many")).unwrap();
        f.journal.capture_before("s", "next").unwrap();
        f.write("a", b"agent");
        f.journal.capture_after("s", "next").unwrap();
        assert_eq!(f.journal.review("s", None).unwrap().changes.len(), 1);
    }

    #[test]
    fn overlapping_turns_remain_unavailable_and_cannot_fake_an_after() {
        let f = Fixture::new();
        f.journal.capture_before("main", "main-t").unwrap();
        f.write("a", b"main");
        assert!(f.journal.capture_before("background", "bg-t").is_err());
        assert_eq!(
            f.journal.review("main", None).unwrap().status,
            "unavailable"
        );
        assert_eq!(
            f.journal.review("background", None).unwrap().status,
            "unavailable"
        );
        f.write("a", b"background");
        assert!(f.journal.capture_after("background", "bg-t").is_err());
        assert!(f.journal.capture_after("main", "main-t").is_err());
        assert!(f.journal.review("main", None).unwrap().changes.is_empty());
        assert!(f.journal.diff("main", "main-t", "a").is_err());
        f.journal.capture_before("main", "fresh").unwrap();
        f.write("a", b"separate turn");
        f.journal.capture_after("main", "fresh").unwrap();
        assert_eq!(
            f.journal
                .diff("main", "fresh", "a")
                .unwrap()
                .before_text
                .as_deref(),
            Some("background")
        );
    }

    #[test]
    fn failed_after_keeps_before_but_never_adopts_later_user_state_on_retry() {
        let f = Fixture::new();
        f.write("a", b"before");
        f.journal.capture_before("s", "t").unwrap();
        for n in 0..MAX_FILES {
            f.write(&format!("many/{n}"), b"x");
        }
        assert!(f.journal.capture_after("s", "t").is_err());
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.status, "unavailable");
        assert!(!review.capture_active);
        assert!(review.capture_error.unwrap().contains("FILE_LIMIT"));
        let conn = f.journal.conn.lock().unwrap();
        let (id, _) = f.journal.require_turn(&conn, "s", "t").unwrap();
        assert_eq!(
            load_file(&conn, id, "before", "a")
                .unwrap()
                .unwrap()
                .content
                .unwrap(),
            b"before"
        );
        assert!(load_snapshot(&conn, id, "after").unwrap().is_empty());
        drop(conn);
        fs::remove_dir_all(f.root.join("many")).unwrap();
        f.write("a", b"after");
        assert!(f.journal.capture_after("s", "t").is_err());
        assert!(f.journal.diff("s", "t", "a").is_err());
        f.journal.capture_before("s", "new-t").unwrap();
        f.write("a", b"new agent edit");
        f.journal.capture_after("s", "new-t").unwrap();
        assert_eq!(
            f.journal
                .diff("s", "new-t", "a")
                .unwrap()
                .before_text
                .as_deref(),
            Some("after")
        );
    }

    #[test]
    fn retention_is_bounded_without_disabling_future_turns_or_changing_legacy() {
        let f = Fixture::new();
        f.legacy("legacy");
        for n in 0..MAX_TURNS + 2 {
            let key = format!("t-{n}");
            f.journal.capture_before("s", &key).unwrap();
            f.write("a", key.as_bytes());
            f.journal.capture_after("s", &key).unwrap();
        }
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.turns.len(), MAX_TURNS);
        assert_eq!(f.journal.list_changes("s", "t").unwrap().len(), 1);
        let count: i64 = f.journal.conn.lock().unwrap().query_row("SELECT count(*) FROM journal_v2_files WHERE turn_id NOT IN (SELECT id FROM journal_v2_turns)", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn total_byte_limit_rolls_back_and_active_turns_block_capture_and_restore() {
        let f = Fixture::new();
        f.journal.capture_before("s", "t").unwrap();
        for n in 0..17 {
            f.write(&format!("big-{n}"), &vec![b'x'; MAX_FILE_BYTES as usize]);
        }
        assert!(f.journal.capture_after("s", "t").is_err());
        let review = f.journal.review("s", None).unwrap();
        assert_eq!(review.status, "unavailable");
        assert!(review.capture_error.unwrap().contains("TOTAL_LIMIT"));
        assert!(!review.capture_active);
        for n in 0..17 {
            fs::remove_file(f.root.join(format!("big-{n}"))).unwrap();
        }
        f.write("a", b"agent");
        f.journal.capture_before("s", "fresh").unwrap();
        f.write("a", b"new agent edit");
        f.journal.capture_after("s", "fresh").unwrap();
        let c = &f.journal.review("s", None).unwrap().changes[0];
        f.journal.capture_before("other", "next").unwrap();
        assert!(f
            .journal
            .restore_file("s", "fresh", "a", &c.expected_after_hash, true)
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_replacement_is_refused_without_blocking_restore() {
        use std::os::unix::fs::OpenOptionsExt;
        let f = Fixture::new();
        f.write("a", b"before");
        f.journal.capture_before("s", "t").unwrap();
        f.write("a", b"agent");
        f.journal.capture_after("s", "t").unwrap();
        let token = f.journal.review("s", None).unwrap().changes[0]
            .expected_after_hash
            .clone();
        fs::remove_file(f.root.join("a")).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(f.root.join("a"))
            .status()
            .unwrap()
            .success());
        let root = f.root.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let journal = FileJournal::open(&root, root.join(".xiaoheng/journal.db")).unwrap();
            send.send(journal.restore_file("s", "t", "a", &token, true).is_err())
                .unwrap();
        });
        let response = receive.recv_timeout(std::time::Duration::from_secs(1));
        if response.is_err() {
            #[cfg(target_os = "linux")]
            let nonblock = 0x800;
            #[cfg(not(target_os = "linux"))]
            let nonblock = 0x4;
            // Release a blocked old implementation so a red test never leaks a
            // worker holding the SQLite transaction or hangs the whole suite.
            let _ = OpenOptions::new()
                .write(true)
                .custom_flags(nonblock)
                .open(f.root.join("a"));
        }
        worker.join().unwrap();
        assert_eq!(
            response.unwrap_or(false),
            true,
            "restore blocked opening a FIFO before validating its type"
        );
    }

    #[test]
    fn host_can_abandon_a_crashed_capture_without_inventing_after_or_blocking_future_turns() {
        let f = Fixture::new();
        f.journal.capture_before("s", "crashed").unwrap();
        f.write("a", b"later user changes");
        f.journal.abandon_capture("s", "crashed").unwrap();
        assert_eq!(f.journal.review("s", None).unwrap().status, "abandoned");
        assert!(f.journal.capture_after("s", "crashed").is_err());
        f.journal.capture_before("s", "fresh").unwrap();
        f.write("a", b"agent");
        f.journal.capture_after("s", "fresh").unwrap();
        assert_eq!(
            f.journal
                .diff("s", "fresh", "a")
                .unwrap()
                .before_text
                .as_deref(),
            Some("later user changes")
        );
        assert!(f.journal.abandon_capture("s", "fresh").is_err());
    }

    #[test]
    fn publication_rechecks_the_moved_inode_and_never_clobbers_recreated_files() {
        let f = Fixture::new();
        f.write("destination", b"later user replacement");
        f.write("staged", b"baseline");
        assert!(publish_protected(
            &f.root.join("destination"),
            Some(&f.root.join("staged")),
            &hash(b"agent after")
        )
        .is_err());
        assert_eq!(
            fs::read(f.root.join("destination")).unwrap(),
            b"later user replacement"
        );
        assert!(publish_protected(
            &f.root.join("destination"),
            Some(&f.root.join("staged")),
            ABSENT
        )
        .is_err());
        assert_eq!(
            fs::read(f.root.join("destination")).unwrap(),
            b"later user replacement"
        );
    }

    #[cfg(unix)]
    #[test]
    fn redirected_journal_storage_is_refused_even_with_aliased_workspace_root() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let outside = Fixture::new();
        let alias = f.root.with_extension("alias");
        symlink(&f.root, &alias).unwrap();
        fs::rename(f.root.join(".xiaoheng"), f.root.join("old-journal")).unwrap();
        symlink(outside.root.join(".xiaoheng"), f.root.join(".xiaoheng")).unwrap();
        let result = FileJournal::open(&alias, alias.join(".xiaoheng/journal.db"));
        fs::remove_file(&alias).unwrap();
        assert!(
            result.is_err(),
            "journal storage cannot follow a workspace-internal symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_parent_and_hardlink_replacements_cannot_escape() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let outside = Fixture::new();
        f.write("dir/a", b"before");
        f.journal.capture_before("s", "t").unwrap();
        f.write("dir/a", b"agent");
        f.journal.capture_after("s", "t").unwrap();
        let c = &f.journal.review("s", None).unwrap().changes[0];
        fs::remove_dir_all(f.root.join("dir")).unwrap();
        outside.write("a", b"agent");
        symlink(&outside.root, f.root.join("dir")).unwrap();
        assert!(f
            .journal
            .restore_file("s", "t", "dir/a", &c.expected_after_hash, true)
            .is_err());
        assert_eq!(fs::read(outside.root.join("a")).unwrap(), b"agent");
        fs::remove_file(f.root.join("dir")).unwrap();
        fs::create_dir(f.root.join("dir")).unwrap();
        fs::hard_link(outside.root.join("a"), f.root.join("dir/a")).unwrap();
        assert!(f
            .journal
            .restore_file("s", "t", "dir/a", &c.expected_after_hash, true)
            .is_err());
        assert!(f.journal.diff("s", "t", "../a").is_err());
        assert!(f.journal.diff("s", "t", "/tmp/a").is_err());
    }
}

/// Workspace-scoped file journal. Cheap to open; backed by a SQLite file.
pub struct FileJournal {
    conn: Mutex<Connection>,
    workspace_root: PathBuf,
    db_path: PathBuf,
}

impl FileJournal {
    /// Open (and migrate) the journal DB. `workspace_root` is the directory
    /// tool paths are resolved against; `db_path` is where snapshots live
    /// (its parent directory is created if missing).
    pub fn open(workspace_root: impl AsRef<Path>, db_path: impl AsRef<Path>) -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let input_root = if workspace_root.as_ref().is_absolute() {
            workspace_root.as_ref().to_path_buf()
        } else {
            cwd.join(workspace_root.as_ref())
        };
        let input_db = if db_path.as_ref().is_absolute() {
            db_path.as_ref().to_path_buf()
        } else {
            cwd.join(db_path.as_ref())
        };
        let workspace_root = fs::canonicalize(&input_root).context("workspace unavailable")?;
        ensure!(workspace_root.is_dir(), "workspace must be a directory");
        let relative_db = input_db
            .strip_prefix(&input_root)
            .or_else(|_| input_db.strip_prefix(&workspace_root))
            .context("UNSAFE_PATH: journal database must be inside the workspace")?;
        // Do not follow redirected .xiaoheng directories or journal databases.
        let db_path = checked_path(
            &workspace_root,
            relative_db.to_str().context("invalid journal path")?,
        )?;
        if let Ok(metadata) = fs::symlink_metadata(&db_path) {
            ensure!(
                metadata.is_file() && !is_hardlinked(&metadata),
                "UNSAFE_PATH: journal database is not a regular unlinked file"
            );
        }
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create journal dir {}", parent.display()))?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("open journal db {}", db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS file_snapshots (
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
                ON file_snapshots(session_id, turn_id);
            CREATE TABLE IF NOT EXISTS journal_v2_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace TEXT NOT NULL, session_id TEXT NOT NULL, turn_key TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'capturing',
                UNIQUE(workspace, session_id, turn_key)
            );
            CREATE TABLE IF NOT EXISTS journal_v2_files (
                turn_id INTEGER NOT NULL, phase TEXT NOT NULL, path TEXT NOT NULL,
                size INTEGER NOT NULL, hash TEXT, binary INTEGER NOT NULL,
                skipped TEXT, content BLOB,
                PRIMARY KEY(turn_id, phase, path)
            );
            CREATE TABLE IF NOT EXISTS journal_v2_restores (
                turn_id INTEGER NOT NULL, path TEXT NOT NULL,
                PRIMARY KEY(turn_id, path)
            );
            CREATE TABLE IF NOT EXISTS journal_v2_capture_state (
                turn_id INTEGER PRIMARY KEY, active INTEGER NOT NULL,
                error TEXT
            );
            INSERT OR IGNORE INTO journal_v2_capture_state(turn_id,active)
                SELECT id, CASE WHEN status='capturing' THEN 1 ELSE 0 END FROM journal_v2_turns;
            COMMIT;",
        )
        .context("journal migrate failed")?;
        Ok(Self {
            conn: Mutex::new(conn),
            workspace_root,
            db_path: fs::canonicalize(&db_path)?,
        })
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
        let _ = (session_id, turn_id);
        bail!("CONFIRMATION_REQUIRED: legacy snapshots have no after hash; automatic undo is disabled. Review and restore one captured file at a time.")
    }
}

impl FileJournal {
    /// Call on a blocking worker BEFORE starting the runtime, using a stable host
    /// turn key. Retrying the same key never replaces the original baseline.
    #[allow(dead_code)] // Host runtime wiring belongs to the parent integration.
    pub fn capture_before(&self, session_id: &str, turn_key: &str) -> Result<()> {
        validate_key(session_id, turn_key)?;
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let workspace = self.workspace_root.to_string_lossy();
        if let Some((_, status)) = turn_row(&tx, &workspace, session_id, turn_key)? {
            ensure!(
                status != "abandoned",
                "CAPTURE_ABANDONED: use a fresh turn key"
            );
            if status == "unavailable" {
                let (id, _) = self.require_turn(&tx, session_id, turn_key)?;
                bail!(
                    "{}",
                    capture_state(&tx, id)?
                        .1
                        .unwrap_or_else(|| "CAPTURE_UNAVAILABLE: use a fresh turn key".into())
                );
            }
            return Ok(());
        }
        let active: i64 = tx.query_row(
            "SELECT count(*) FROM journal_v2_turns t JOIN journal_v2_capture_state c ON c.turn_id=t.id WHERE t.workspace=?1 AND c.active=1",
            params![workspace],
            |r| r.get(0),
        )?;
        // Additive v2 retention never deletes or rewrites legacy snapshots.
        let mut stmt = tx.prepare("SELECT t.id FROM journal_v2_turns t JOIN journal_v2_capture_state c ON c.turn_id=t.id WHERE t.workspace=?1 AND c.active=0 ORDER BY t.id DESC LIMIT -1 OFFSET ?2")?;
        let evicted = stmt
            .query_map(params![workspace, (MAX_TURNS - 1) as i64], |r| {
                r.get::<_, i64>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for id in evicted {
            tx.execute("DELETE FROM journal_v2_files WHERE turn_id=?1", params![id])?;
            tx.execute(
                "DELETE FROM journal_v2_restores WHERE turn_id=?1",
                params![id],
            )?;
            tx.execute("DELETE FROM journal_v2_turns WHERE id=?1", params![id])?;
            tx.execute(
                "DELETE FROM journal_v2_capture_state WHERE turn_id=?1",
                params![id],
            )?;
        }
        tx.execute(
            "INSERT INTO journal_v2_turns(workspace,session_id,turn_key) VALUES (?1,?2,?3)",
            params![workspace, session_id, turn_key],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO journal_v2_capture_state(turn_id,active) VALUES (?1,1)",
            params![id],
        )?;
        if active > 0 {
            let reason = "CAPTURE_BUSY: overlapping workspace writers; changes cannot be safely attributed. Chat may continue, but review/restore is unavailable for overlapping turns.";
            tx.execute("UPDATE journal_v2_turns SET status='unavailable' WHERE workspace=?1 AND id IN (SELECT turn_id FROM journal_v2_capture_state WHERE active=1)", params![workspace])?;
            tx.execute("UPDATE journal_v2_capture_state SET error=COALESCE(error,?1) WHERE active=1 AND turn_id IN (SELECT id FROM journal_v2_turns WHERE workspace=?2)", params![reason, workspace])?;
            tx.commit()?;
            bail!("{reason}");
        }
        let snapshot = match self.scan() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let reason = error.to_string();
                record_unavailable(&tx, id, &reason, true)?;
                tx.commit()?;
                bail!("{reason}");
            }
        };
        store_snapshot(&tx, id, "before", &snapshot)?;
        tx.commit()?;
        Ok(())
    }

    /// Call after terminal success/failure/cancellation AND after all workspace
    /// writers have stopped. No after snapshot is synthesized without a baseline.
    #[allow(dead_code)] // Host runtime wiring belongs to the parent integration.
    pub fn capture_after(&self, session_id: &str, turn_key: &str) -> Result<()> {
        validate_key(session_id, turn_key)?;
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (id, status) = self.require_turn(&tx, session_id, turn_key)?;
        if status == "complete" {
            return Ok(());
        }
        if status == "unavailable" {
            let reason = capture_state(&tx, id)?
                .1
                .unwrap_or_else(|| "CAPTURE_UNAVAILABLE".into());
            tx.execute(
                "UPDATE journal_v2_capture_state SET active=0 WHERE turn_id=?1",
                params![id],
            )?;
            tx.commit()?;
            bail!("{reason}");
        }
        ensure!(
            status == "capturing",
            "CAPTURE_ABANDONED: cannot attach an after snapshot to an abandoned turn"
        );
        let snapshot = match self.scan() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let reason = error.to_string();
                record_unavailable(&tx, id, &reason, false)?;
                tx.commit()?;
                bail!("{reason}");
            }
        };
        store_snapshot(&tx, id, "after", &snapshot)?;
        tx.execute(
            "UPDATE journal_v2_turns SET status='complete' WHERE id=?1",
            params![id],
        )?;
        tx.execute(
            "UPDATE journal_v2_capture_state SET active=0 WHERE turn_id=?1",
            params![id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Host-only recovery hook, after verifying that no runtime writers remain.
    #[allow(dead_code)]
    pub fn abandon_capture(&self, session_id: &str, turn_key: &str) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (id, status) = self.require_turn(&tx, session_id, turn_key)?;
        ensure!(
            status != "complete",
            "CAPTURE_COMPLETE: completed review cannot be abandoned"
        );
        if status != "unavailable" {
            tx.execute(
                "UPDATE journal_v2_turns SET status='abandoned' WHERE id=?1",
                params![id],
            )?;
        }
        tx.execute("UPDATE journal_v2_capture_state SET active=0, error=COALESCE(error,'CAPTURE_ABANDONED: no trustworthy terminal snapshot') WHERE turn_id=?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Invalidate a capture WITHOUT asserting that writers have stopped. The
    /// host must still finish/abandon after true quiescence to release guards.
    #[allow(dead_code)]
    pub fn mark_capture_unavailable(
        &self,
        session_id: &str,
        turn_key: &str,
        reason: &str,
    ) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (id, status) = self.require_turn(&tx, session_id, turn_key)?;
        ensure!(
            status == "capturing" || status == "unavailable",
            "CAPTURE_CLOSED: completed/abandoned capture cannot be reopened"
        );
        let active = capture_state(&tx, id)?.0;
        record_unavailable(
            &tx,
            id,
            &reason.chars().take(2048).collect::<String>(),
            active,
        )?;
        tx.commit()?;
        Ok(())
    }

    fn require_turn(&self, conn: &Connection, session: &str, key: &str) -> Result<(i64, String)> {
        validate_key(session, key)?;
        turn_row(conn, &self.workspace_root.to_string_lossy(), session, key)?
            .context("TURN_NOT_FOUND: no baseline for this session, turn and workspace")
    }

    /// Metadata only: never loads all captured content or computes all diffs.
    pub fn review(&self, session_id: &str, turn_key: Option<&str>) -> Result<JournalReview> {
        validate_key(session_id, turn_key.unwrap_or("latest"))?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let mut stmt = conn.prepare("SELECT turn_key,status FROM journal_v2_turns WHERE workspace=?1 AND session_id=?2 ORDER BY id DESC LIMIT ?3")?;
        let turns = stmt
            .query_map(
                params![
                    self.workspace_root.to_string_lossy(),
                    session_id,
                    MAX_TURNS as i64
                ],
                |r| {
                    Ok(ReviewTurn {
                        turn_key: r.get(0)?,
                        status: r.get(1)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let selected = turn_key
            .map(str::to_owned)
            .or_else(|| turns.first().map(|t| t.turn_key.clone()));
        let Some(key) = selected else {
            return Ok(JournalReview {
                turn_key: None,
                status: "empty".into(),
                capture_error: None,
                capture_active: false,
                turns,
                changes: vec![],
                warnings: vec![],
            });
        };
        let (id, status) = self.require_turn(&conn, session_id, &key)?;
        let (capture_active, capture_error) = capture_state(&conn, id)?;
        let before = load_snapshot(&conn, id, "before")?;
        let after = load_snapshot(&conn, id, "after")?;
        let skipped = before
            .values()
            .chain(after.values())
            .filter(|f| f.summary.skipped.is_some())
            .count();
        let mut warnings = vec![];
        if skipped > 0 {
            warnings.push(format!("{skipped} snapshot entries were skipped (large, linked or special files); they cannot be restored."));
        }
        if status == "unavailable" {
            warnings.push(capture_error.clone().unwrap_or_else(|| {
                "CAPTURE_UNAVAILABLE: no safe review or restore is available".into()
            }));
        } else if status == "abandoned" {
            warnings.push("Capture was abandoned by the host after execution stopped; no after snapshot or restore is available. Start a new turn with a fresh key.".into());
        } else if status != "complete" {
            warnings.push("After snapshot pending or failed. Finish/retry capture after the runtime has stopped; restore is disabled.".into());
        }
        let mut changes = vec![];
        if status == "complete" {
            let paths: BTreeSet<_> = before.keys().chain(after.keys()).cloned().collect();
            for path in paths {
                let b = before.get(&path);
                let a = after.get(&path);
                if same_snapshot(b, a) {
                    continue;
                }
                let restored = conn
                    .query_row(
                        "SELECT 1 FROM journal_v2_restores WHERE turn_id=?1 AND path=?2",
                        params![id, path],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                let supported = b.into_iter().chain(a).all(|f| f.summary.skipped.is_none());
                changes.push(ReviewChange {
                    path,
                    kind: if b.is_none() {
                        "added"
                    } else if a.is_none() {
                        "deleted"
                    } else {
                        "modified"
                    }
                    .into(),
                    before: b.map(|f| f.summary.clone()),
                    after: a.map(|f| f.summary.clone()),
                    expected_after_hash: a
                        .and_then(|f| f.summary.hash.clone())
                        .unwrap_or_else(|| ABSENT.into()),
                    can_restore: supported && !restored,
                    restored,
                });
            }
        }
        Ok(JournalReview {
            turn_key: Some(key),
            status,
            capture_error,
            capture_active,
            turns,
            changes,
            warnings,
        })
    }

    pub fn diff(&self, session: &str, key: &str, path: &str) -> Result<JournalDiff> {
        validate_relative(path)?;
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let (id, status) = self.require_turn(&conn, session, key)?;
        ensure!(
            status == "complete",
            "CAPTURE_PENDING: the turn has not finished"
        );
        let before = load_file(&conn, id, "before", path)?;
        let after = load_file(&conn, id, "after", path)?;
        ensure!(
            !same_snapshot(before.as_ref(), after.as_ref()),
            "CHANGE_NOT_FOUND: refresh the change list"
        );
        let mut out = JournalDiff {
            kind: "text".into(),
            before_text: None,
            after_text: None,
            before: before.as_ref().map(|f| f.summary.clone()),
            after: after.as_ref().map(|f| f.summary.clone()),
            message: None,
        };
        let entries: Vec<_> = before.iter().chain(after.iter()).collect();
        if entries.iter().any(|f| f.summary.skipped.is_some()) {
            out.kind = "unavailable".into();
            out.message =
                Some("File snapshot was skipped; no safe diff or restore is available.".into());
        } else if entries.iter().any(|f| f.summary.binary) {
            out.kind = "binary".into();
            out.message = Some("Binary file: only hashes and byte sizes are available.".into());
        } else if entries.iter().any(|f| {
            f.summary.size > MAX_DIFF_BYTES as u64
                || f.content
                    .as_ref()
                    .is_some_and(|b| b.iter().filter(|c| **c == b'\n').count() > 4000)
        }) {
            out.kind = "too_large".into();
            out.message =
                Some("Text exceeds the diff display limit (128 KiB / 4000 lines per side).".into());
        } else {
            out.before_text = Some(snapshot_text(before.as_ref())?);
            out.after_text = Some(snapshot_text(after.as_ref())?);
        }
        Ok(out)
    }

    /// Explicit confirmation plus the UI's immutable after token are mandatory.
    /// Re-hashes the live file immediately before mutation; never force-restores.
    pub fn restore_file(
        &self,
        session: &str,
        key: &str,
        path: &str,
        expected_after_hash: &str,
        confirmed: bool,
    ) -> Result<()> {
        ensure!(
            confirmed,
            "CONFIRMATION_REQUIRED: explicitly confirm this single-file restore"
        );
        validate_relative(path)?;
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("journal lock poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (id, status) = self.require_turn(&tx, session, key)?;
        ensure!(
            status == "complete",
            "CAPTURE_PENDING: cannot restore an active turn"
        );
        let active: i64 = tx.query_row(
            "SELECT count(*) FROM journal_v2_turns t JOIN journal_v2_capture_state c ON c.turn_id=t.id WHERE t.workspace=?1 AND c.active=1",
            params![self.workspace_root.to_string_lossy()],
            |r| r.get(0),
        )?;
        ensure!(
            active == 0,
            "CAPTURE_BUSY: wait for workspace execution to finish before restoring"
        );
        let before = load_file(&tx, id, "before", path)?;
        let after = load_file(&tx, id, "after", path)?;
        ensure!(
            !same_snapshot(before.as_ref(), after.as_ref()),
            "CHANGE_NOT_FOUND: refresh the change list"
        );
        ensure!(
            before
                .iter()
                .chain(after.iter())
                .all(|f| f.summary.skipped.is_none()),
            "UNSUPPORTED: skipped files cannot be restored"
        );
        ensure!(
            tx.query_row(
                "SELECT 1 FROM journal_v2_restores WHERE turn_id=?1 AND path=?2",
                params![id, path],
                |_| Ok(())
            )
            .optional()?
            .is_none(),
            "ALREADY_RESTORED: refresh the change list"
        );
        let recorded = after
            .as_ref()
            .and_then(|f| f.summary.hash.as_deref())
            .unwrap_or(ABSENT);
        ensure!(
            recorded == expected_after_hash,
            "STALE_REVIEW: refresh and review the file again"
        );
        let abs = checked_path(&self.workspace_root, path)?;
        self.verify_current(path, recorded)?;
        // Lock cooperating journal instances through SQLite and cooperating file
        // writers through fs2. External editors are checked again at publish time.
        let locked = match open_regular_file(&abs) {
            Ok(file) => {
                // Windows byte-range locks also block reads from our other
                // verification handles. A shared lock permits these reads while
                // still denying writes; SQLite serializes journal restorers.
                #[cfg(windows)]
                let result = fs2::FileExt::try_lock_shared(&file);
                #[cfg(not(windows))]
                let result = fs2::FileExt::try_lock_exclusive(&file);
                result.context("FILE_BUSY: close the file and retry")?;
                Some(file)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        self.verify_current(path, recorded)?;
        match before.as_ref() {
            Some(file) => {
                let bytes = file
                    .content
                    .as_ref()
                    .context("SNAPSHOT_MISSING: restore disabled")?;
                ensure!(
                    file.summary.hash.as_deref() == Some(&hash(bytes)),
                    "SNAPSHOT_CORRUPT: restore disabled"
                );
                let parent = abs.parent().context("invalid destination")?;
                // Missing parent directories are not recreated blindly: the user
                // may have removed/replaced them since the after snapshot.
                ensure!(
                    parent.is_dir(),
                    "PARENT_MISSING: recreate the original parent directory, refresh and retry"
                );
                let temp = parent.join(format!(".harmony-restore-{}", uuid::Uuid::new_v4()));
                let result = (|| -> Result<()> {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&temp)?;
                    output.write_all(bytes)?;
                    if let Some(current) = &locked {
                        output.set_permissions(current.metadata()?.permissions())?;
                    }
                    output.sync_all()?;
                    self.verify_current(path, recorded)?;
                    publish_protected(&abs, Some(&temp), recorded)?;
                    Ok(())
                })();
                let _ = fs::remove_file(&temp);
                result?;
            }
            None => {
                self.verify_current(path, recorded)?;
                publish_protected(&abs, None, recorded)?;
            }
        }
        tx.execute(
            "INSERT INTO journal_v2_restores(turn_id,path) VALUES (?1,?2)",
            params![id, path],
        )?;
        tx.commit()
            .context("File restored, but journal update failed. Refresh before retrying.")?;
        Ok(())
    }

    fn verify_current(&self, path: &str, expected: &str) -> Result<()> {
        let abs = checked_path(&self.workspace_root, path)?;
        let current = match fs::symlink_metadata(&abs) {
            Ok(_) => {
                let file = read_snapshot_file(&abs)?;
                ensure!(
                    file.summary.skipped.is_none(),
                    "UNSUPPORTED: destination is linked, special or oversized"
                );
                file.summary.hash.context("missing current hash")?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ABSENT.into(),
            Err(e) => return Err(e.into()),
        };
        ensure!(current == expected, "CONFLICT: file changed after this turn; your later edits were preserved. Refresh and compare manually.");
        Ok(())
    }

    fn scan(&self) -> Result<Snapshot> {
        checked_root(&self.workspace_root)?;
        let mut files = Snapshot::new();
        let mut pending = vec![(self.workspace_root.clone(), 0usize)];
        let mut entries = 0;
        let mut total = 0;
        while let Some((dir, depth)) = pending.pop() {
            ensure!(depth <= 64, "SCAN_LIMIT: directory depth exceeds 64");
            checked_path(
                &self.workspace_root,
                dir.strip_prefix(&self.workspace_root)?
                    .to_str()
                    .context("non-UTF8 path")?,
            )?;
            for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name
                    .to_str()
                    .context("UNSUPPORTED_PATH: workspace contains a non-UTF8 file name")?;
                if ignored(name) || name.starts_with(".harmony-restore-") {
                    continue;
                }
                entries += 1;
                ensure!(
                    entries <= MAX_SCAN_ENTRIES,
                    "SCAN_LIMIT: more than {MAX_SCAN_ENTRIES} entries"
                );
                let path = entry.path();
                if path == self.db_path
                    || path
                        .to_string_lossy()
                        .starts_with(&format!("{}-", self.db_path.display()))
                {
                    continue;
                }
                let rel = path
                    .strip_prefix(&self.workspace_root)?
                    .to_str()
                    .context("non-UTF8 path")?
                    .replace(std::path::MAIN_SEPARATOR, "/");
                validate_relative(&rel)?;
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.is_dir() && !is_redirected(&metadata) {
                    pending.push((path, depth + 1));
                } else {
                    ensure!(
                        files.len() < MAX_FILES,
                        "FILE_LIMIT: more than {MAX_FILES} files; narrow the workspace"
                    );
                    checked_path(&self.workspace_root, &rel).or_else(|e| {
                        if is_redirected(&metadata) {
                            Ok(path.clone())
                        } else {
                            Err(e)
                        }
                    })?;
                    let file = read_snapshot_file(&path)?;
                    total += file.content.as_ref().map_or(0, Vec::len);
                    ensure!(
                        total <= MAX_TOTAL_BYTES,
                        "TOTAL_LIMIT: snapshot exceeds 16 MiB; narrow the workspace"
                    );
                    files.insert(rel, file);
                }
            }
        }
        Ok(files)
    }
}

fn validate_key(session: &str, key: &str) -> Result<()> {
    ensure!(
        !session.trim().is_empty()
            && !key.trim().is_empty()
            && session.len() <= 256
            && key.len() <= 256,
        "INVALID_KEY: nonempty session/turn keys (max 256 bytes) required"
    );
    Ok(())
}

// Move the live name aside and verify the actual moved inode before publishing
// with an atomic no-clobber link. A writer recreating the destination wins;
// rollback never overwrites that writer, and reports the preserved backup path.
fn publish_protected(destination: &Path, replacement: Option<&Path>, expected: &str) -> Result<()> {
    if expected == ABSENT {
        let source = replacement.context("SNAPSHOT_MISSING")?;
        fs::hard_link(source, destination)
            .context("CONFLICT: destination was recreated or atomic publication is unsupported")?;
        return Ok(());
    }
    let backup = destination
        .parent()
        .context("invalid destination")?
        .join(format!(".harmony-restore-{}", uuid::Uuid::new_v4()));
    fs::rename(destination, &backup).context("CONFLICT: destination changed before restore")?;
    let attempt = (|| -> Result<()> {
        let moved = read_snapshot_file(&backup)?;
        ensure!(
            moved.summary.hash.as_deref() == Some(expected) && moved.summary.skipped.is_none(),
            "CONFLICT: file changed immediately before restore"
        );
        if let Some(source) = replacement {
            fs::hard_link(source, destination).context(
                "CONFLICT: destination was recreated or atomic publication is unsupported",
            )?;
        }
        let latest = read_snapshot_file(&backup)?;
        ensure!(
            latest.summary.hash.as_deref() == Some(expected),
            "CONFLICT: original file was edited while restore was publishing"
        );
        Ok(())
    })();
    if let Err(error) = attempt {
        if fs::hard_link(&backup, destination).is_ok() {
            let _ = fs::remove_file(&backup);
            return Err(error);
        }
        bail!("{error}; original file preserved at {}. A new destination was not overwritten; compare both files manually.", backup.display());
    }
    fs::remove_file(&backup).with_context(|| {
        format!(
            "Restored destination, but backup cleanup failed: {}",
            backup.display()
        )
    })?;
    Ok(())
}

fn ignored(name: &str) -> bool {
    [
        ".git",
        ".hg",
        ".svn",
        "node_modules",
        ".xiaoheng",
        "target",
        "dist",
        "build",
        ".next",
        ".venv",
        "venv",
        "__pycache__",
    ]
    .iter()
    .any(|n| name.eq_ignore_ascii_case(n))
}

fn validate_relative(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && !path.contains(['\\', ':', '\0']) && !Path::new(path).is_absolute(),
        "UNSAFE_PATH: relative workspace file required"
    );
    ensure!(
        path.split('/').all(|p| !p.is_empty()
            && p != "."
            && p != ".."
            && !ignored(p)
            && !p.ends_with(['.', ' '])),
        "UNSAFE_PATH: ignored or non-normal workspace path"
    );
    ensure!(
        Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "UNSAFE_PATH"
    );
    Ok(())
}

fn checked_root(root: &Path) -> Result<()> {
    ensure!(
        !is_redirected(&fs::symlink_metadata(root)?) && fs::canonicalize(root)? == root,
        "WORKSPACE_MOVED: workspace root changed; reopen the session"
    );
    Ok(())
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    // Internal DB paths may include .xiaoheng, unlike review/restore paths.
    ensure!(
        !Path::new(relative).is_absolute()
            && Path::new(relative)
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "UNSAFE_PATH"
    );
    checked_root(root)?;
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(m) => ensure!(
                !is_redirected(&m),
                "UNSAFE_PATH: symbolic links/reparse points are not supported"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}

fn is_redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn is_hardlinked(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.nlink() > 1
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn open_regular_file(path: &Path) -> Result<File, std::io::Error> {
    let mut options = OpenOptions::new();
    options.read(true);
    // Nonblocking open closes the metadata-check/FIFO-swap race, and no-follow
    // (or Windows OPEN_REPARSE_POINT) rejects a redirected leaf at open time.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x0004 | 0x0100);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x0800 | 0x20000);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_redirected(&metadata) || is_hardlinked(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "UNSUPPORTED: only regular unlinked files may be opened",
        ));
    }
    Ok(file)
}

fn same_file_identity(first: &fs::Metadata, second: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        first.dev() == second.dev() && first.ino() == second.ino()
    }
    #[cfg(not(unix))]
    {
        first.len() == second.len() && first.modified().ok() == second.modified().ok()
    }
}

fn read_snapshot_file(path: &Path) -> Result<SnapshotFile> {
    let metadata = fs::symlink_metadata(path)?;
    let reason = if is_redirected(&metadata) {
        Some("symbolic link/reparse point")
    } else if !metadata.is_file() {
        Some("special file")
    } else if is_hardlinked(&metadata) {
        Some("hard-linked file")
    } else if metadata.len() > MAX_FILE_BYTES {
        Some("file exceeds 1 MiB")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Ok(SnapshotFile {
            summary: FileSummary {
                size: metadata.len(),
                hash: None,
                binary: false,
                skipped: Some(reason.into()),
            },
            content: None,
        });
    }
    let file = open_regular_file(path)?;
    let opened = file.metadata()?;
    ensure!(
        same_file_identity(&metadata, &opened),
        "FILE_CHANGED_DURING_CAPTURE: opened file identity changed"
    );
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    let latest = fs::symlink_metadata(path)?;
    ensure!(
        bytes.len() as u64 == metadata.len()
            && bytes.len() as u64 <= MAX_FILE_BYTES
            && metadata.modified()? == latest.modified()?
            && same_file_identity(&opened, &latest)
            && !is_redirected(&latest)
            && !is_hardlinked(&latest),
        "FILE_CHANGED_DURING_CAPTURE: retry when workspace writers have stopped"
    );
    let binary = bytes.contains(&0) || std::str::from_utf8(&bytes).is_err();
    Ok(SnapshotFile {
        summary: FileSummary {
            size: bytes.len() as u64,
            hash: Some(hash(&bytes)),
            binary,
            skipped: None,
        },
        content: Some(bytes),
    })
}

fn turn_row(
    conn: &Connection,
    workspace: &str,
    session: &str,
    key: &str,
) -> Result<Option<(i64, String)>> {
    Ok(conn.query_row("SELECT id,status FROM journal_v2_turns WHERE workspace=?1 AND session_id=?2 AND turn_key=?3", params![workspace,session,key], |r| Ok((r.get(0)?,r.get(1)?))).optional()?)
}

fn capture_state(conn: &Connection, id: i64) -> Result<(bool, Option<String>)> {
    Ok(conn.query_row(
        "SELECT active,error FROM journal_v2_capture_state WHERE turn_id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

fn record_unavailable(conn: &Connection, id: i64, reason: &str, active: bool) -> Result<()> {
    conn.execute(
        "UPDATE journal_v2_turns SET status='unavailable' WHERE id=?1",
        params![id],
    )?;
    conn.execute(
        "UPDATE journal_v2_capture_state SET active=?1,error=?2 WHERE turn_id=?3",
        params![active, reason, id],
    )?;
    Ok(())
}

fn store_snapshot(conn: &Connection, id: i64, phase: &str, snapshot: &Snapshot) -> Result<()> {
    let mut insert = conn.prepare("INSERT INTO journal_v2_files(turn_id,phase,path,size,hash,binary,skipped,content) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)")?;
    for (path, file) in snapshot {
        insert.execute(params![
            id,
            phase,
            path,
            file.summary.size as i64,
            file.summary.hash,
            file.summary.binary,
            file.summary.skipped,
            file.content
        ])?;
    }
    Ok(())
}

fn snapshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SnapshotFile> {
    Ok(SnapshotFile {
        summary: FileSummary {
            size: row.get::<_, i64>(0)? as u64,
            hash: row.get(1)?,
            binary: row.get(2)?,
            skipped: row.get(3)?,
        },
        content: row.get(4)?,
    })
}

fn load_snapshot(conn: &Connection, id: i64, phase: &str) -> Result<Snapshot> {
    let mut stmt = conn.prepare("SELECT size,hash,binary,skipped,NULL,path FROM journal_v2_files WHERE turn_id=?1 AND phase=?2")?;
    let rows = stmt.query_map(params![id, phase], |r| {
        Ok((r.get::<_, String>(5)?, snapshot_row(r)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Snapshot>>()?)
}

fn load_file(conn: &Connection, id: i64, phase: &str, path: &str) -> Result<Option<SnapshotFile>> {
    Ok(conn.query_row("SELECT size,hash,binary,skipped,content FROM journal_v2_files WHERE turn_id=?1 AND phase=?2 AND path=?3", params![id,phase,path], snapshot_row).optional()?)
}

fn same_snapshot(before: Option<&SnapshotFile>, after: Option<&SnapshotFile>) -> bool {
    match (before, after) {
        (None, None) => true,
        (Some(b), Some(a)) => {
            b.summary.hash == a.summary.hash
                && b.summary.size == a.summary.size
                && b.summary.skipped == a.summary.skipped
        }
        _ => false,
    }
}

fn snapshot_text(file: Option<&SnapshotFile>) -> Result<String> {
    match file {
        None => Ok(String::new()),
        Some(file) => Ok(String::from_utf8(
            file.content.clone().context("SNAPSHOT_MISSING")?,
        )?),
    }
}
