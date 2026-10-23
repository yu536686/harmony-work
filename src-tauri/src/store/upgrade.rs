use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BACKUP_DIR: &str = "backups/macos-memory-pre-1.1.2";
const MARKER: &str = ".macos-memory-1.1.2-initialized";
const MARKER_CONTENT: &str = "macos-memory-1.1.2\n";

/// Keep the lock through initialization and marker publication. A failed or
/// interrupted migration must never replace the original pre-upgrade snapshot.
pub(super) fn initialize<T>(db_path: &Path, initialize: impl FnOnce() -> Result<T>) -> Result<T> {
    let root = db_path
        .parent()
        .context("Database has no app-data directory")?;
    let lock = private_file(&root.join(".macos-memory-1.1.2.lock"), false)?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("Another macOS memory upgrade is initializing; retry after it finishes")?;

    let marker = root.join(MARKER);
    if regular_file_exists(&marker)? {
        if fs::read_to_string(&marker)? != MARKER_CONTENT {
            bail!(
                "Invalid macOS memory upgrade marker at {}",
                marker.display()
            );
        }
        return initialize();
    }

    // Preflight marker creation/writes before allowing any schema changes.
    let pending = TemporaryPath(
        root.join(format!("{MARKER}-{}", uuid::Uuid::new_v4())),
        false,
    );
    let mut marker_file = private_file(&pending.0, true)
        .context("Cannot prepare macOS memory upgrade marker; database was not migrated")?;
    marker_file.write_all(MARKER_CONTENT.as_bytes())?;
    marker_file.sync_all()?;
    drop(marker_file);
    sync_directory(root)?;

    if regular_file_exists(db_path)? {
        prepare_backup(db_path, root)?;
    }

    let value = initialize()?;
    fs::rename(&pending.0, &marker).context("Cannot publish macOS memory upgrade marker")?;
    sync_directory(root).context("Cannot persist macOS memory upgrade marker")?;
    Ok(value)
}

fn prepare_backup(db_path: &Path, root: &Path) -> Result<()> {
    let backups = root.join("backups");
    match fs::symlink_metadata(&backups) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => bail!("Invalid backup root at {}", backups.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => private_directory(&backups)?,
        Err(error) => return Err(error).context("Cannot inspect backup root"),
    }
    let published = root.join(BACKUP_DIR);
    match fs::symlink_metadata(&published) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                bail!(
                    "Invalid macOS memory backup directory at {}",
                    published.display()
                );
            }
            // Only complete, atomically published snapshots are reusable on retry.
            let snapshot = published.join("workbuddy.db");
            verify_snapshot(&snapshot)?;
            File::open(snapshot)?.sync_all()?;
            sync_directory(&published)?;
            sync_directory(&backups)?;
            return sync_directory(root);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Cannot inspect macOS memory backup directory"),
    }

    let staging = TemporaryPath(
        backups.join(format!(".macos-memory-pre-1.1.2-{}", uuid::Uuid::new_v4())),
        true,
    );
    private_directory(&staging.0)?;
    let snapshot = staging.0.join("workbuddy.db");
    let snapshot_path = snapshot
        .to_str()
        .context("Backup path is not valid UTF-8")?;
    let source = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("Cannot open existing database for macOS memory backup")?;
    source.busy_timeout(Duration::from_secs(5))?;
    // SQLite reads a consistent snapshot including committed, uncheckpointed WAL.
    // Never copy the live database or its WAL/SHM files independently.
    source
        .execute("VACUUM INTO ?1", [snapshot_path])
        .context("Cannot back up existing database; macOS memory migration was not started")?;
    drop(source);
    verify_snapshot(&snapshot)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o600))?;
    }
    File::open(&snapshot)?.sync_all()?;
    sync_directory(&staging.0)?;
    fs::rename(&staging.0, &published).context("Cannot publish macOS memory backup")?;
    sync_directory(&backups)?;
    sync_directory(root)?;
    Ok(())
}

fn verify_snapshot(path: &Path) -> Result<()> {
    if !regular_file_exists(path)? {
        bail!("Missing macOS memory backup at {}", path.display());
    }
    if fs::metadata(path)?.len() == 0 {
        bail!("Empty macOS memory backup at {}", path.display());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        bail!(
            "Invalid macOS memory backup at {}: {result}",
            path.display()
        );
    }
    Ok(())
}

fn regular_file_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => bail!("Expected a regular file at {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("Cannot inspect {}", path.display())),
    }
}

fn private_file(path: &Path, create_new: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .create_new(create_new);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("Cannot create {}", path.display()))
}

fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("Cannot create {}", path.display()))
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

struct TemporaryPath(PathBuf, bool);

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        if self.1 {
            let _ = fs::remove_dir_all(&self.0);
        } else {
            let _ = fs::remove_file(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("harmony-upgrade-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn db(&self) -> PathBuf {
            self.0.join("workbuddy.db")
        }

        fn backup(&self) -> PathBuf {
            self.0.join(BACKUP_DIR).join("workbuddy.db")
        }

        fn marker(&self) -> PathBuf {
            self.0.join(MARKER)
        }

        fn seed_wal(&self) -> Connection {
            let conn = Connection::open(self.db()).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
                 CREATE TABLE memories (content TEXT);
                 INSERT INTO memories VALUES ('committed in WAL');",
            )
            .unwrap();
            assert!(fs::metadata(self.0.join("workbuddy.db-wal")).unwrap().len() > 0);
            conn
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn contents(path: &Path) -> String {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT content FROM memories", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn backs_up_committed_wal_before_initialization_without_copying_other_data() {
        let root = TempRoot::new();
        let live = root.seed_wal();
        for name in ["config.json", "secrets.json", "key", "workspace"] {
            fs::write(root.0.join(name), format!("unchanged {name}")).unwrap();
        }
        initialize(&root.db(), || {
            assert!(!root.marker().exists());
            assert_eq!(contents(&root.backup()), "committed in WAL");
            live.execute_batch(
                "UPDATE memories SET content='after migration'; CREATE TABLE migrated (id TEXT);",
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(contents(&root.db()), "after migration");
        assert_eq!(contents(&root.backup()), "committed in WAL");
        assert_eq!(fs::read_to_string(root.marker()).unwrap(), MARKER_CONTENT);
        assert!(!root.backup().with_extension("db-wal").exists());
        for name in ["config.json", "secrets.json", "key", "workspace"] {
            assert_eq!(
                fs::read_to_string(root.0.join(name)).unwrap(),
                format!("unchanged {name}")
            );
        }
    }

    #[test]
    fn fresh_install_has_no_backup_even_on_next_launch() {
        let root = TempRoot::new();
        initialize(&root.db(), || {
            assert!(!root.marker().exists());
            Connection::open(root.db())?.execute_batch("CREATE TABLE fresh (id TEXT)")?;
            Ok(())
        })
        .unwrap();
        initialize(&root.db(), || Ok(())).unwrap();
        assert!(root.marker().is_file());
        assert!(!root.0.join("backups").exists());
    }

    #[test]
    fn failed_initialization_has_no_marker_and_retry_keeps_original_snapshot() {
        let root = TempRoot::new();
        let live = root.seed_wal();
        assert!(initialize(&root.db(), || -> Result<()> {
            live.execute_batch("UPDATE memories SET content='partial migration'")?;
            anyhow::bail!("migration failed")
        })
        .is_err());
        assert!(!root.marker().exists());
        let original = fs::read(root.backup()).unwrap();
        initialize(&root.db(), || Ok(())).unwrap();
        live.execute_batch("UPDATE memories SET content='later launch'")
            .unwrap();
        initialize(&root.db(), || Ok(())).unwrap();
        assert_eq!(fs::read(root.backup()).unwrap(), original);
        assert_eq!(contents(&root.backup()), "committed in WAL");
    }

    #[test]
    fn backup_failure_never_calls_initializer() {
        let root = TempRoot::new();
        let _live = root.seed_wal();
        fs::write(root.0.join("backups"), "blocked").unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
        assert!(!root.marker().exists());
        assert_eq!(contents(&root.db()), "committed in WAL");
    }

    #[test]
    fn invalid_source_or_backup_never_calls_initializer() {
        let root = TempRoot::new();
        fs::write(root.db(), "not sqlite").unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
        assert!(!root.backup().exists());
        fs::remove_file(root.db()).unwrap();
        let _live = root.seed_wal();
        fs::create_dir_all(root.0.join(BACKUP_DIR)).unwrap();
        fs::write(root.backup(), "interrupted snapshot").unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
        assert!(!root.marker().exists());
    }

    #[test]
    fn marker_preflight_failure_never_calls_initializer() {
        let root = TempRoot::new();
        let _live = root.seed_wal();
        fs::create_dir(root.marker()).unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
        fs::remove_dir(root.marker()).unwrap();
        fs::write(root.marker(), "invalid marker").unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
    }

    #[test]
    fn marker_publication_failure_is_returned_and_backup_survives_retry() {
        let root = TempRoot::new();
        let live = root.seed_wal();
        assert!(initialize(&root.db(), || {
            live.execute_batch("UPDATE memories SET content='migrated'")?;
            fs::create_dir(root.marker())?;
            Ok(())
        })
        .is_err());
        assert_eq!(contents(&root.backup()), "committed in WAL");
        fs::remove_dir(root.marker()).unwrap();
        initialize(&root.db(), || Ok(())).unwrap();
        assert_eq!(contents(&root.backup()), "committed in WAL");
    }

    #[test]
    fn concurrent_initialization_fails_closed() {
        let root = TempRoot::new();
        let _live = root.seed_wal();
        initialize(&root.db(), || {
            assert!(
                initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn real_database_migrations_preserve_memory_sessions_identity_and_tombstones() {
        let root = TempRoot::new();
        let source = Connection::open(root.db()).unwrap();
        source.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
             CREATE TABLE memories (
                 id TEXT PRIMARY KEY, content TEXT NOT NULL, category TEXT NOT NULL,
                 confidence REAL NOT NULL, source_session_id TEXT,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             INSERT INTO memories VALUES ('memory-107', 'keep my preference', 'preference', 0.9,
                 'session-107', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY, title TEXT, status TEXT NOT NULL,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
                 message_count INTEGER NOT NULL DEFAULT 0, workspace_root TEXT
             );
             INSERT INTO sessions VALUES ('session-107', 'keep my session', 'idle',
                 '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z', 0, '/keep/workspace');
             CREATE TABLE memory_sync_identity (singleton INTEGER PRIMARY KEY, user_id TEXT NOT NULL);
             INSERT INTO memory_sync_identity VALUES (1, 'keep-account');
             CREATE TABLE memory_tombstones (id TEXT PRIMARY KEY, deleted_at TEXT NOT NULL);
             INSERT INTO memory_tombstones VALUES ('deleted-memory', '2026-09-01T00:00:00Z');"
        ).unwrap();
        let db = initialize(&root.db(), || crate::store::Database::open(&root.db())).unwrap();
        let memories = db.list_memories().unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, "memory-107");
        assert_eq!(memories[0].content, "keep my preference");
        assert_eq!(
            db.get_session("session-107")
                .unwrap()
                .unwrap()
                .workspace_root
                .as_deref(),
            Some("/keep/workspace")
        );
        for path in [root.db(), root.backup()] {
            let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert_eq!(
                conn.query_row("SELECT user_id FROM memory_sync_identity", [], |row| row
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
                "keep-account"
            );
            assert_eq!(
                conn.query_row("SELECT id FROM memory_tombstones", [], |row| row
                    .get::<_, String>(0))
                    .unwrap(),
                "deleted-memory"
            );
        }
        let backup =
            Connection::open_with_flags(root.backup(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let columns: i64 = backup
            .query_row(
                "SELECT count(*) FROM pragma_table_info('memories') WHERE name='kind'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 0, "backup must contain the pre-migration schema");
        assert!(root.marker().is_file());
    }

    #[test]
    fn backup_path_handles_spaces_and_sql_quotes() {
        let parent = TempRoot::new();
        let root = TempRoot(parent.0.join("user's app data"));
        fs::create_dir(&root.0).unwrap();
        let _live = root.seed_wal();
        initialize(&root.db(), || Ok(())).unwrap();
        assert_eq!(contents(&root.backup()), "committed in WAL");
    }

    #[test]
    fn snapshot_includes_only_committed_transactions() {
        let root = TempRoot::new();
        let live = root.seed_wal();
        live.execute_batch("BEGIN; INSERT INTO memories VALUES ('uncommitted');")
            .unwrap();
        initialize(&root.db(), || Ok(())).unwrap();
        let backup =
            Connection::open_with_flags(root.backup(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let count: i64 = backup
            .query_row("SELECT count(*) FROM memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(contents(&root.backup()), "committed in WAL");
        live.execute_batch("ROLLBACK").unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn backup_is_private_and_symlinked_backup_root_is_rejected() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = TempRoot::new();
        let _live = root.seed_wal();
        initialize(&root.db(), || Ok(())).unwrap();
        assert_eq!(
            fs::metadata(root.backup()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.0.join(BACKUP_DIR))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        fs::remove_file(root.marker()).unwrap();
        fs::rename(root.0.join("backups"), root.0.join("outside")).unwrap();
        symlink(root.0.join("outside"), root.0.join("backups")).unwrap();
        assert!(initialize(&root.db(), || -> Result<()> { panic!("must not migrate") }).is_err());
    }
}
