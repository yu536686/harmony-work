//! macOS bundle transactions; deliberately independent of the plugin installer.

use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    ffi::{c_char, c_int, c_void, CString},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const APP_NAME: &str = "Harmony Work.app";
const BUNDLE_ID: &str = "com.harmony.xiaoheng-workbuddy";
const EXECUTABLE: &str = "harmony-work";
const EXECUTABLE_REL: &str = "Contents/MacOS/harmony-work";
const HELPER: &str = "update-helper";
const ARCHIVE: &str = "payload.app.tar.gz";
const MANIFEST: &str = "manifest.json";
const HEALTH: &str = "health.json";
const READY: &str = "ready.json";
const BACKUP: &str = "previous.app";
const FAILED: &str = "failed.app";
pub(super) const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 16 * 1024;
const MAX_EXTENSION_BYTES: u64 = 64 * 1024;
// macOS O_NOFOLLOW; Rust already adds close-on-exec to opened descriptors.
const O_NOFOLLOW: c_int = 0x100;

extern "C" {
    fn geteuid() -> u32;
    fn kill(pid: c_int, signal: c_int) -> c_int;
    fn hw_update_helper_progress(
        phase: *const c_char,
        detail: *const c_char,
        failed: bool,
        finished: bool,
    );
}
#[link(name = "proc")]
extern "C" {
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
}

fn progress(phase: &str, detail: &str, failed: bool, finished: bool) {
    let phase = CString::new(phase.replace('\0', " ")).unwrap();
    let detail = CString::new(detail.replace('\0', " ")).unwrap();
    // run_helper is synchronous on main(), before Tauri creates its event loop.
    // The native bridge must pump AppKit's run loop on each invocation.
    unsafe { hw_update_helper_progress(phase.as_ptr(), detail.as_ptr(), failed, finished) };
}

#[derive(Clone, Copy)]
struct Limits {
    entries: u64,
    file_bytes: u64,
    expanded_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 250_000,
            file_bytes: 4 * 1024 * 1024 * 1024,
            expanded_bytes: 16 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    original: PathBuf,
    old_version: String,
    version: String,
    old_pid: u32,
    token: String,
    archive_sha256: String,
    created_at: u64,
}
impl Manifest {
    fn new(original: PathBuf, old_version: &str, version: &str, archive_sha256: String) -> Self {
        Self {
            schema: 1,
            original,
            old_version: old_version.into(),
            version: version.into(),
            old_pid: std::process::id(),
            token: uuid::Uuid::new_v4().simple().to_string(),
            archive_sha256,
            created_at: super::now(),
        }
    }
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new().mode(0o700).create(path).map_err(|e| {
        format!(
            "Cannot create private update directory {}: {e}",
            path.display()
        )
    })
}
fn open_regular(path: &Path, max: u64) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() > max {
        return Err(format!(
            "Invalid or oversized update file: {}",
            path.display()
        ));
    }
    Ok(file)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let mut data = Vec::new();
    open_regular(path, MAX_JSON_BYTES)?
        .take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_JSON_BYTES {
        return Err("Update JSON exceeds limit".into());
    }
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn write_manifest(tx: &Path, manifest: &Manifest) -> Result<(), String> {
    let bytes = serde_json::to_vec(manifest).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err("Update manifest exceeds limit".into());
    }
    write_private(&tx.join(MANIFEST), &bytes)
}
fn hash_file(path: &Path, max: u64) -> Result<String, String> {
    let mut file = open_regular(path, max)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 128 * 1024];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > max {
            return Err("Update file exceeds limit".into());
        }
        hash.update(&buf[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn transaction_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix(".harmony-update-"))
        .is_some_and(|s| is_hex(s, 32))
}
fn canonical_exact(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || path.to_string_lossy().len() > 4096
        || path.canonicalize().map_err(|e| e.to_string())? != path
    {
        return Err(format!("Update path is not canonical: {}", path.display()));
    }
    Ok(())
}
fn validate_manifest(tx: &Path, m: &Manifest, helper: Option<&Path>) -> Result<(), String> {
    canonical_exact(tx)?;
    let meta = fs::symlink_metadata(tx).map_err(|e| e.to_string())?;
    if !meta.is_dir()
        || meta.uid() != unsafe { geteuid() }
        || meta.mode() & 0o777 != 0o700
        || !transaction_name(tx)
    {
        return Err("Update transaction must be a private, owned sibling directory".into());
    }
    if m.schema != 1
        || !is_hex(&m.token, 32)
        || !is_hex(&m.archive_sha256, 64)
        || m.old_pid <= 1
        || m.old_pid > i32::MAX as u32
        || m.old_version.len() > 128
        || m.version.len() > 128
        || m.created_at > super::now().saturating_add(60)
        || super::now().saturating_sub(m.created_at) > 86_400
    {
        return Err("Invalid or expired update manifest".into());
    }
    let old = semver::Version::parse(&m.old_version).map_err(|e| e.to_string())?;
    let new = semver::Version::parse(&m.version).map_err(|e| e.to_string())?;
    if new <= old {
        return Err("Update manifest must advance the installed version".into());
    }
    canonical_exact(&m.original)?;
    if m.original.file_name().and_then(|s| s.to_str()) != Some(APP_NAME)
        || m.original.parent() != tx.parent()
        || fs::symlink_metadata(&m.original)
            .map_err(|e| e.to_string())?
            .dev()
            != meta.dev()
    {
        return Err("Update transaction is not on the installed bundle's volume and parent".into());
    }
    if let Some(helper) = helper {
        canonical_exact(helper)?;
        if helper != tx.join(HELPER) {
            return Err("Update helper is outside its transaction".into());
        }
        open_regular(helper, MAX_ARCHIVE_BYTES)?;
    }
    Ok(())
}

fn bundle_from_exe(exe: &Path) -> Result<PathBuf, String> {
    canonical_exact(exe)?;
    let bundle = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("Not a macOS app executable")?;
    if bundle.file_name().and_then(|s| s.to_str()) != Some(APP_NAME)
        || exe != bundle.join(EXECUTABLE_REL)
    {
        return Err("Run Harmony Work from its installed .app bundle before updating".into());
    }
    Ok(bundle.to_path_buf())
}
fn validate_bundle(bundle: &Path, version: &str) -> Result<(), String> {
    canonical_exact(bundle)?;
    let contents = bundle.join("Contents");
    let plist = contents.join("Info.plist");
    canonical_exact(&contents)?;
    canonical_exact(&plist)?;
    open_regular(&plist, 1024 * 1024)?;
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&plist)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Cannot validate app Info.plist: {e}"))?;
    if !output.status.success() || output.stdout.len() > 4 * 1024 * 1024 {
        return Err("Invalid app Info.plist".into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    for (key, expected) in [
        ("CFBundleIdentifier", BUNDLE_ID),
        ("CFBundleShortVersionString", version),
        ("CFBundleExecutable", EXECUTABLE),
        ("CFBundlePackageType", "APPL"),
    ] {
        if json.get(key).and_then(|v| v.as_str()) != Some(expected) {
            return Err(format!("Unexpected app {key}; expected {expected}"));
        }
    }
    let exe = bundle.join(EXECUTABLE_REL);
    canonical_exact(&exe)?;
    let file = open_regular(&exe, MAX_ARCHIVE_BYTES)?;
    if file.metadata().map_err(|e| e.to_string())?.mode() & 0o111 == 0 {
        return Err("App executable is not executable".into());
    }
    Ok(())
}

fn safe_archive_path(value: &Path) -> Result<PathBuf, String> {
    let text = value.to_str().ok_or("Archive path is not UTF-8")?;
    if text.len() > 4096 || text.contains(['\\', ':']) || text.chars().any(char::is_control) {
        return Err("Unsafe archive path".into());
    }
    let mut result = PathBuf::new();
    for component in value.components() {
        match component {
            Component::Normal(part) => result.push(part),
            Component::CurDir => {}
            _ => return Err("Absolute or traversing archive path".into()),
        }
    }
    if result.components().next().map(|c| c.as_os_str()) != Some(std::ffi::OsStr::new(APP_NAME)) {
        return Err("Archive must contain only Harmony Work.app".into());
    }
    Ok(result)
}
fn safe_link(path: &Path, target: &Path) -> Result<(), String> {
    let text = target.to_str().ok_or("Archive link is not UTF-8")?;
    if text.is_empty()
        || text.len() > 4096
        || text.contains(['\\', ':'])
        || text.chars().any(char::is_control)
    {
        return Err("Unsafe archive link".into());
    }
    let mut resolved = path.parent().ok_or("Invalid archive link")?.to_path_buf();
    for component in target.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved.components().count() <= 1 {
                    return Err("Archive link escapes app bundle".into());
                }
                resolved.pop();
            }
            _ => return Err("Absolute archive link".into()),
        }
    }
    if !resolved.starts_with(APP_NAME) {
        return Err("Archive link escapes app bundle".into());
    }
    Ok(())
}

// Raw tar iteration lets us bound GNU/PAX metadata before tar allocates it.
// Links are created only after every regular file, so no write follows a link.
fn extract_archive(archive: &Path, destination: &Path, limits: Limits) -> Result<PathBuf, String> {
    private_dir(destination)?;
    let decoder = MultiGzDecoder::new(open_regular(archive, MAX_ARCHIVE_BYTES)?);
    let stream_limit = limits
        .expanded_bytes
        .saturating_add(limits.entries.saturating_mul(2048));
    let mut archive = tar::Archive::new(decoder.take(stream_limit.saturating_add(1)));
    let mut seen = HashSet::new();
    let mut links = Vec::new();
    let mut count = 0u64;
    let mut expanded = 0u64;
    let mut next_path: Option<PathBuf> = None;
    let mut next_link: Option<PathBuf> = None;
    let mut directory_modes = Vec::new();
    for entry in archive.entries().map_err(|e| e.to_string())?.raw(true) {
        let mut entry = entry.map_err(|e| e.to_string())?;
        count += 1;
        if count > limits.entries {
            return Err("Update archive has too many entries".into());
        }
        let kind = entry.header().entry_type();
        let size = entry.size();
        if kind.is_gnu_longname() || kind.is_gnu_longlink() {
            if size > MAX_EXTENSION_BYTES {
                return Err("Oversized tar path metadata".into());
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            let path = PathBuf::from(String::from_utf8(bytes).map_err(|e| e.to_string())?);
            let slot = if kind.is_gnu_longname() {
                &mut next_path
            } else {
                &mut next_link
            };
            if slot.replace(path).is_some() {
                return Err("Duplicate tar path metadata".into());
            }
            continue;
        }
        if kind.is_pax_local_extensions() {
            if size > MAX_EXTENSION_BYTES {
                return Err("Oversized PAX metadata".into());
            }
            if let Some(fields) = entry.pax_extensions().map_err(|e| e.to_string())? {
                for field in fields {
                    let field = field.map_err(|e| e.to_string())?;
                    let key = field.key().map_err(|e| e.to_string())?;
                    let value = field.value().map_err(|e| e.to_string())?;
                    match key {
                        "path" | "linkpath" => {
                            let slot = if key == "path" {
                                &mut next_path
                            } else {
                                &mut next_link
                            };
                            if slot.replace(PathBuf::from(value)).is_some() {
                                return Err("Duplicate PAX path metadata".into());
                            }
                        }
                        "size" => return Err("PAX size overrides are not supported".into()),
                        key if key.starts_with("GNU.sparse") => {
                            return Err("Sparse archive entries are forbidden".into())
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }
        if !(kind.is_file() || kind.is_dir() || kind.is_symlink()) {
            return Err("Archive contains a hardlink or special entry".into());
        }
        let path = match next_path.take() {
            Some(p) => p,
            None => entry.path().map_err(|e| e.to_string())?.into_owned(),
        };
        let relative = safe_archive_path(&path)?;
        let target = destination.join(&relative);
        if !seen.insert(relative.clone()) {
            return Err("Duplicate archive entry".into());
        }
        if size > limits.file_bytes {
            return Err("Archive entry exceeds size limit".into());
        }
        expanded = expanded.checked_add(size).ok_or("Archive size overflow")?;
        if expanded > limits.expanded_bytes {
            return Err("Archive exceeds expanded size limit".into());
        }
        let link = next_link.take();
        if kind.is_symlink() {
            if size != 0 {
                return Err("Symlink contains unexpected data".into());
            }
            let link = match link {
                Some(p) => p,
                None => entry
                    .link_name()
                    .map_err(|e| e.to_string())?
                    .ok_or("Missing archive link target")?
                    .into_owned(),
            };
            safe_link(&relative, &link)?;
            links.push((target, link));
        } else {
            if link.is_some() {
                return Err("Link metadata on a regular archive entry".into());
            }
            if kind.is_dir() {
                if size != 0 {
                    return Err("Directory contains unexpected data".into());
                }
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                directory_modes.push((
                    target,
                    entry.header().mode().map_err(|e| e.to_string())? & 0o777,
                ));
            } else {
                fs::create_dir_all(target.parent().ok_or("Invalid archive file")?)
                    .map_err(|e| e.to_string())?;
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&target)
                    .map_err(|e| e.to_string())?;
                let copied = io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
                if copied != size {
                    return Err("Truncated archive entry".into());
                }
                let mode = entry.header().mode().map_err(|e| e.to_string())? & 0o777;
                file.set_permissions(fs::Permissions::from_mode(mode))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    if next_path.is_some() || next_link.is_some() {
        return Err("Dangling archive metadata".into());
    }
    let mut tail = archive.into_inner();
    let mut buffer = [0; 8192];
    loop {
        let n = tail.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if buffer[..n].iter().any(|b| *b != 0) {
            return Err("Unexpected trailing archive data".into());
        }
    }
    if tail.limit() == 0 {
        return Err("Archive stream exceeds limit".into());
    }
    let bundle = destination.join(APP_NAME);
    if !bundle.is_dir() {
        return Err("Archive contains no app bundle".into());
    }
    for (path, link) in &links {
        let parent = path.parent().ok_or("Invalid symlink parent")?;
        // Do not traverse even an internal link while creating a later link.
        let mut ancestor = parent;
        while ancestor != destination {
            if let Ok(meta) = fs::symlink_metadata(ancestor) {
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("Archive symlink has a symlink parent".into());
                }
            }
            ancestor = ancestor.parent().ok_or("Invalid symlink parent")?;
        }
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // A link may never replace an existing file, directory, or earlier link.
        std::os::unix::fs::symlink(link, path).map_err(|e| e.to_string())?;
    }
    for (path, _) in &links {
        if !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(&bundle)
        {
            return Err("Resolved archive symlink escapes bundle".into());
        }
    }
    directory_modes.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, mode) in directory_modes {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    }
    Ok(bundle)
}

fn installation_lock(parent: &Path) -> Result<File, String> {
    let path = parent.join(".harmony-work-update.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("The app's parent is not writable: {e}"))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file()
        || meta.uid() != unsafe { geteuid() }
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
    {
        return Err("Unsafe installation lock".into());
    }
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| "Another Harmony Work installation is running".to_string())?;
    // Do not unlink lock files: another process may already hold this inode.
    Ok(file)
}

pub(super) struct Prepared {
    transaction: PathBuf,
    manifest: Manifest,
    lock: Option<File>,
    cleanup_on_drop: bool,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = fs::remove_dir_all(&self.transaction);
        }
    }
}

/// Called only with the bytes returned by the pinned plugin's verified download.
pub(super) fn prepare(bytes: &[u8], old_version: &str, version: &str) -> Result<Prepared, String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("Update archive exceeds size limit".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let original = bundle_from_exe(&exe)?;
    validate_bundle(&original, old_version)?;
    let parent = original.parent().ok_or("Missing app parent")?;
    let lock = installation_lock(parent)?;
    let transaction = parent.join(format!(".harmony-update-{}", uuid::Uuid::new_v4().simple()));
    // This also preflights the actual writable parent; no cross-volume temp dirs.
    private_dir(&transaction)?;
    let manifest = Manifest::new(
        original,
        old_version,
        version,
        hex::encode(Sha256::digest(bytes)),
    );
    let prepared = Prepared {
        transaction,
        manifest,
        lock: Some(lock),
        cleanup_on_drop: true,
    };
    validate_manifest(&prepared.transaction, &prepared.manifest, None)?;
    write_private(&prepared.transaction.join(ARCHIVE), bytes)?;
    let staged = extract_archive(
        &prepared.transaction.join(ARCHIVE),
        &prepared.transaction.join("staged"),
        Limits::default(),
    )?;
    validate_bundle(&staged, version)?;
    let helper = prepared.transaction.join(HELPER);
    fs::copy(&exe, &helper).map_err(|e| format!("Cannot copy update helper: {e}"))?;
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    File::open(&helper)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    write_manifest(&prepared.transaction, &prepared.manifest)?;
    File::open(&prepared.transaction)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(prepared)
}

impl Prepared {
    /// Wait for authenticated helper readiness before shutting down the old UI.
    pub(super) fn handoff(mut self) -> Result<(), String> {
        let mut child = Command::new(self.transaction.join(HELPER))
            .arg("--harmony-macos-update")
            .arg(&self.transaction)
            .current_dir(
                self.transaction
                    .parent()
                    .ok_or("Missing transaction parent")?,
            )
            .env_remove("HARMONY_UPDATE_HEALTH_FILE")
            .env_remove("HARMONY_UPDATE_HEALTH_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start update helper: {e}"))?;
        self.cleanup_on_drop = false;
        drop(self.lock.take());
        let deadline = Instant::now() + Duration::from_secs(1800);
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                let detail = read_json::<serde_json::Value>(&self.transaction.join("failure.json"))
                    .ok()
                    .and_then(|v| v.get("error").and_then(|s| s.as_str()).map(str::to_owned))
                    .unwrap_or_else(|| format!("Update helper exited before handoff: {status}"));
                return Err(detail);
            }
            if let Ok(ready) = read_json::<serde_json::Value>(&self.transaction.join(READY)) {
                if ready["token"] == self.manifest.token
                    && ready["version"] == self.manifest.version
                {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                child
                    .kill()
                    .map_err(|e| format!("Helper handoff timed out; cannot stop helper: {e}"))?;
                let _ = child.wait();
                return Err(
                    "Update helper handoff timed out; the installed app was not changed".into(),
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn swap_with(
    original: &Path,
    staged: &Path,
    backup: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), String> {
    match fs::symlink_metadata(backup) {
        Ok(_) => return Err("Update backup already exists; refusing to overwrite it".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    rename(original, backup).map_err(|e| format!("Cannot back up installed app: {e}"))?;
    if let Err(error) = rename(staged, original) {
        return match rename(backup, original) {
            Ok(()) => Err(format!("Cannot install app; original restored: {error}")),
            Err(restore) => Err(format!(
                "Cannot install app: {error}; original backup retained at {}: {restore}",
                backup.display()
            )),
        };
    }
    Ok(())
}
fn swap_bundle(original: &Path, staged: &Path, backup: &Path) -> Result<(), String> {
    swap_with(original, staged, backup, |from, to| fs::rename(from, to))
}
fn rollback_bundle(original: &Path, backup: &Path, failed: &Path) -> Result<(), String> {
    if !backup.is_dir() {
        return Err("Original backup is unavailable; no update files were removed".into());
    }
    if fs::symlink_metadata(failed).is_ok() {
        return Err("Failed bundle already exists; backup retained".into());
    }
    fs::rename(original, failed)
        .map_err(|e| format!("Cannot move failed app; original backup retained: {e}"))?;
    fs::rename(backup, original).map_err(|e| {
        format!(
            "Cannot restore original app; backup retained at {}: {e}",
            backup.display()
        )
    })
}

fn process_alive(pid: u32) -> bool {
    // EPERM means alive too; an inaccessible process must never be replaced.
    unsafe { kill(pid as c_int, 0) == 0 || io::Error::last_os_error().raw_os_error() != Some(3) }
}
fn process_image(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut bytes = vec![0u8; 4096];
    let length =
        unsafe { proc_pidpath(pid as c_int, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if length <= 0 {
        return None;
    }
    bytes.truncate(
        bytes
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(length as usize),
    );
    Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
}
fn wait_for_old(m: &Manifest, notify: &impl Fn(&str, &str)) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    while process_alive(m.old_pid) {
        if let Some(image) = process_image(m.old_pid) {
            if image != m.original.join(EXECUTABLE_REL) {
                return Err("The old process identity changed; refusing bundle replacement".into());
            }
        }
        if Instant::now() >= deadline {
            return Err("The old Harmony Work process did not exit; app left unchanged".into());
        }
        notify("waiting", "Waiting for Harmony Work to close");
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
fn launch(original: &Path, health: Option<(&Path, &str)>) -> Result<Child, String> {
    let mut command = Command::new(original.join(EXECUTABLE_REL));
    command
        .current_dir(original.parent().ok_or("Missing app parent")?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("HARMONY_UPDATE_HEALTH_FILE")
        .env_remove("HARMONY_UPDATE_HEALTH_TOKEN");
    if let Some((path, token)) = health {
        command
            .env("HARMONY_UPDATE_HEALTH_FILE", path)
            .env("HARMONY_UPDATE_HEALTH_TOKEN", token);
    }
    command
        .spawn()
        .map_err(|e| format!("Cannot start Harmony Work: {e}"))
}
fn await_health(
    child: &mut Child,
    tx: &Path,
    m: &Manifest,
    notify: &impl Fn(&str, &str),
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut healthy_since = None;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "Updated app exited before health confirmation: {status}"
            ));
        }
        if let Ok(health) = read_json::<serde_json::Value>(&tx.join(HEALTH)) {
            if health["token"] == m.token && health["version"] == m.version {
                let first = healthy_since.get_or_insert_with(Instant::now);
                // A health write followed immediately by a process crash is not success.
                if first.elapsed() >= Duration::from_secs(2) {
                    return Ok(());
                }
            } else {
                return Err("Updated app sent an invalid health token or version".into());
            }
        }
        if Instant::now() >= deadline {
            return Err("Updated frontend did not confirm health in time".into());
        }
        notify("restarting", "Waiting for the updated app to become ready");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn helper_worker(tx: &Path, notify: impl Fn(&str, &str)) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let m: Manifest = read_json(&tx.join(MANIFEST))?;
    validate_manifest(tx, &m, Some(&exe))?;
    if m.old_pid == std::process::id() {
        return Err("Update helper cannot replace itself as the old app".into());
    }
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(10);
        let _lock = loop {
            match installation_lock(tx.parent().ok_or("Missing transaction parent")?) {
                Ok(lock) => break lock,
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        notify("verifying", "Verifying the signed update transaction");
        validate_bundle(&m.original, &m.old_version)?;
        if hash_file(&exe, MAX_ARCHIVE_BYTES)?
            != hash_file(&m.original.join(EXECUTABLE_REL), MAX_ARCHIVE_BYTES)?
        {
            return Err("Update helper does not match the installed executable".into());
        }
        if hash_file(&tx.join(ARCHIVE), MAX_ARCHIVE_BYTES)? != m.archive_sha256 {
            return Err("Update archive hash changed after signature verification".into());
        }
        // Re-extract the authenticated archive rather than trusting a staged tree
        // that could have changed while the old app was waiting for idle jobs.
        let staged = extract_archive(&tx.join(ARCHIVE), &tx.join("verified"), Limits::default())?;
        validate_bundle(&staged, &m.version)?;
        if fs::symlink_metadata(tx.join(HEALTH)).is_ok() {
            return Err("Unexpected pre-existing update health file".into());
        }
        let ready =
            serde_json::to_vec(&serde_json::json!({"token": m.token, "version": m.version}))
                .map_err(|e| e.to_string())?;
        write_private(&tx.join(READY), &ready)?;
        wait_for_old(&m, &notify)?;
        validate_bundle(&m.original, &m.old_version)?;
        validate_bundle(&staged, &m.version)?;
        let backup = tx.join(BACKUP);
        notify("installing", "Installing Harmony Work");
        if let Err(error) = swap_bundle(&m.original, &staged, &backup) {
            // swap_bundle restores automatically if its second rename failed.
            // If restoration itself failed, leave the backup untouched.
            if validate_bundle(&m.original, &m.old_version).is_ok() {
                let _ = launch(&m.original, None);
            }
            return Err(error);
        }
        let startup = match launch(&m.original, Some((&tx.join(HEALTH), &m.token))) {
            Err(error) => Err(error),
            Ok(mut child) => {
                match await_health(&mut child, tx, &m, &notify) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        // Do not rename a bundle out from under a child we could
                        // not stop. Keep the original backup in that case.
                        if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                            if let Err(stop) = child.kill().and_then(|_| child.wait().map(|_| ())) {
                                return Err(format!("{error}; cannot stop updated app, backup retained at {}: {stop}", backup.display()));
                            }
                        }
                        Err(error)
                    }
                }
            }
        };
        if let Err(error) = startup {
            notify("rollback", "Restoring the previous Harmony Work app");
            rollback_bundle(&m.original, &backup, &tx.join(FAILED))
                .map_err(|restore| format!("{error}; {restore}"))?;
            validate_bundle(&m.original, &m.old_version)?;
            return match launch(&m.original, None) {
                Ok(_) => Err(format!(
                    "{error}; previous app restored and restarted. Transaction retained at {}",
                    tx.display()
                )),
                Err(restart) => Err(format!(
                    "{error}; previous app restored, but restart failed: {restart}"
                )),
            };
        }
        notify("cleanup", "Update confirmed; cleaning temporary files");
        // Backup removal is strictly after token/version/child health success.
        // Cleanup errors do not undo a healthy update or discard failure evidence.
        if fs::remove_dir_all(&backup).is_ok() {
            let _ = fs::remove_dir_all(tx);
        }
        Ok(())
    })();
    if let Err(error) = &result {
        let bounded: String = error.chars().take(4096).collect();
        if let Ok(bytes) = serde_json::to_vec(&serde_json::json!({"error": bounded})) {
            let _ = write_private(&tx.join("failure.json"), &bytes);
        }
    }
    result
}

/// Binary entry point. AppKit calls stay on this thread; disk work and process
/// waits run on a worker so even multi-GB archive verification cannot freeze UI.
pub fn run_helper(transaction: &Path) -> i32 {
    enum Event {
        Phase(String, String),
        Done(Result<(), String>),
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let tx = transaction.to_path_buf();
    let worker = std::thread::spawn(move || {
        let result = helper_worker(&tx, |phase, detail| {
            let _ = sender.send(Event::Phase(phase.into(), detail.into()));
        });
        let _ = sender.send(Event::Done(result));
    });
    let mut phase = "verifying".to_string();
    let mut detail = "Preparing Harmony Work update".to_string();
    let result = loop {
        progress(&phase, &detail, false, false);
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::Phase(p, d)) => {
                phase = p;
                detail = d;
            }
            Ok(Event::Done(result)) => break result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                break Err(
                    "Update helper stopped unexpectedly; transaction and backup retained".into(),
                )
            }
        }
    };
    let _ = worker.join();
    match result {
        Ok(()) => {
            progress("finished", "Harmony Work has been updated", false, true);
            0
        }
        Err(error) => {
            progress("error", &error, true, true);
            1
        }
    }
}

pub(super) fn valid_health_target(exe: &Path, target: &Path, token: &str, version: &str) -> bool {
    (|| -> Result<(), String> {
        let bundle = bundle_from_exe(exe)?;
        if !target.is_absolute() || target.file_name().and_then(|s| s.to_str()) != Some(HEALTH) {
            return Err("Invalid health target".into());
        }
        let tx = target.parent().ok_or("Missing health transaction")?;
        let manifest: Manifest = read_json(&tx.join(MANIFEST))?;
        validate_manifest(tx, &manifest, None)?;
        if manifest.original != bundle || manifest.token != token || manifest.version != version {
            return Err("Health does not match this bundle transaction".into());
        }
        if let Ok(meta) = fs::symlink_metadata(target) {
            if !meta.is_file() || meta.file_type().is_symlink() || meta.nlink() != 1 {
                return Err("Unsafe health target".into());
            }
        }
        Ok(())
    })()
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Cursor;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .canonicalize()
                .unwrap()
                .join(format!("harmony-mac-test-{}", uuid::Uuid::new_v4()));
            private_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn health_requires_matching_token_and_live_process() {
        let temp = Temp::new();
        let manifest = Manifest::new(temp.0.join(APP_NAME), "1.1.1", "1.1.2", "0".repeat(64));
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        fs::write(
            temp.0.join(HEALTH),
            serde_json::to_vec(&serde_json::json!({
                "token": manifest.token, "version": manifest.version
            }))
            .unwrap(),
        )
        .unwrap();
        let result = await_health(&mut child, &temp.0, &manifest, &|_, _| {});
        let _ = child.kill();
        let _ = child.wait();
        assert!(result.is_ok());
    }
    #[test]
    fn health_rejects_foreign_token() {
        let temp = Temp::new();
        let manifest = Manifest::new(temp.0.join(APP_NAME), "1.1.1", "1.1.2", "0".repeat(64));
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        fs::write(
            temp.0.join(HEALTH),
            br#"{"token":"wrong","version":"1.1.2"}"#,
        )
        .unwrap();
        let result = await_health(&mut child, &temp.0, &manifest, &|_, _| {});
        let _ = child.kill();
        let _ = child.wait();
        assert!(result.unwrap_err().contains("invalid health token"));
    }
    #[test]
    fn health_rejects_app_that_exits_before_confirmation() {
        let temp = Temp::new();
        let manifest = Manifest::new(temp.0.join(APP_NAME), "1.1.1", "1.1.2", "0".repeat(64));
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        fs::write(
            temp.0.join(HEALTH),
            serde_json::to_vec(&serde_json::json!({
                "token": manifest.token, "version": manifest.version
            }))
            .unwrap(),
        )
        .unwrap();
        let result = await_health(&mut child, &temp.0, &manifest, &|_, _| {});
        let _ = child.wait();
        assert!(result
            .unwrap_err()
            .contains("exited before health confirmation"));
    }
    fn archive(entries: &[(&str, u8, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (name, kind, value) in entries {
            let mut header = tar::Header::new_gnu();
            // Set raw names so the fixture can represent hostile paths too.
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_entry_type(tar::EntryType::new(*kind));
            header.set_mode(0o755);
            if *kind == b'2' || *kind == b'1' {
                header.set_link_name(value).unwrap();
                header.set_size(0);
            } else {
                header.set_size(value.len() as u64);
            }
            header.set_cksum();
            let data = if *kind == b'2' || *kind == b'1' {
                ""
            } else {
                value
            };
            builder
                .append(&header, Cursor::new(data.as_bytes()))
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }
    fn extract_fixture(root: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
        let payload = root.join("payload.app.tar.gz");
        fs::write(&payload, bytes).unwrap();
        extract_archive(&payload, &root.join("staged"), Limits::default())
    }
    fn minimal_bundle(root: &Path, version: &str) -> PathBuf {
        let app = root.join(APP_NAME);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>
            <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
            <key>CFBundleShortVersionString</key><string>{version}</string>
            <key>CFBundleExecutable</key><string>{EXECUTABLE}</string>
            <key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#
        );
        fs::write(app.join("Contents/Info.plist"), plist).unwrap();
        fs::write(app.join(EXECUTABLE_REL), b"test executable").unwrap();
        fs::set_permissions(app.join(EXECUTABLE_REL), fs::Permissions::from_mode(0o755)).unwrap();
        app
    }
    #[test]
    fn archive_rejects_traversal_absolute_and_foreign_roots() {
        for path in [
            "../escape",
            "/tmp/escape",
            "Harmony Work.app/../escape",
            "Other.app/Contents/file",
            "Harmony Work.app/Contents/../../escape",
            "Harmony Work.app/Contents/back\\slash",
            "Harmony Work.app/Contents/C:/file",
        ] {
            let temp = Temp::new();
            assert!(
                extract_fixture(&temp.0, &archive(&[(path, b'0', "bad")])).is_err(),
                "{path}"
            );
            assert!(!temp.0.join("escape").exists());
        }
    }
    #[test]
    fn archive_rejects_escaping_links_and_all_hardlinks() {
        for (kind, target) in [
            (b'2', "/tmp"),
            (b'2', "../../outside"),
            (b'1', "Harmony Work.app/file"),
        ] {
            let temp = Temp::new();
            assert!(extract_fixture(
                &temp.0,
                &archive(&[("Harmony Work.app/link", kind, target)])
            )
            .is_err());
        }
    }
    #[test]
    fn archive_cannot_write_through_a_symlink_or_alias_duplicate() {
        let temp = Temp::new();
        let bytes = archive(&[
            ("Harmony Work.app/link", b'2', "Contents"),
            ("Harmony Work.app/link/file", b'0', "bad"),
        ]);
        assert!(extract_fixture(&temp.0, &bytes).is_err());
        let temp = Temp::new();
        let bytes = archive(&[
            ("Harmony Work.app/Contents/file", b'0', "first"),
            ("./Harmony Work.app/Contents/file", b'0', "second"),
        ]);
        assert!(extract_fixture(&temp.0, &bytes).is_err());
    }
    #[test]
    fn delayed_links_cannot_create_a_write_through_shorter_symlink_parent() {
        let temp = Temp::new();
        let bytes = archive(&[
            ("Harmony Work.app/Contents/file", b'0', "content"),
            ("Harmony Work.app/a", b'2', "."),
            ("Harmony Work.app/a/b", b'2', "../outside"),
            ("Harmony Work.app/a/b/sub/link", b'2', "target"),
        ]);
        assert!(extract_fixture(&temp.0, &bytes).is_err());
        assert!(!temp.0.join("staged/outside").exists());
    }
    #[test]
    fn archive_preserves_only_confined_framework_symlinks() {
        let temp = Temp::new();
        let bytes = archive(&[
            (
                "Harmony Work.app/Contents/Frameworks/F.framework/Versions/A/F",
                b'0',
                "framework",
            ),
            (
                "Harmony Work.app/Contents/Frameworks/F.framework/Versions/Current",
                b'2',
                "A",
            ),
            (
                "Harmony Work.app/Contents/Frameworks/F.framework/F",
                b'2',
                "Versions/Current/F",
            ),
        ]);
        let bundle = extract_fixture(&temp.0, &bytes).unwrap();
        assert_eq!(
            fs::read(bundle.join("Contents/Frameworks/F.framework/F")).unwrap(),
            b"framework"
        );
    }
    #[test]
    fn extracted_minimal_app_validates_against_expected_release() {
        let temp = Temp::new();
        let source = minimal_bundle(&temp.0.join("source"), "1.1.2");
        let plist = fs::read_to_string(source.join("Contents/Info.plist")).unwrap();
        let bytes = archive(&[
            ("Harmony Work.app/Contents/Info.plist", b'0', &plist),
            (
                "Harmony Work.app/Contents/MacOS/harmony-work",
                b'0',
                "test executable",
            ),
        ]);
        let extracted = extract_fixture(&temp.0, &bytes).unwrap();
        assert!(validate_bundle(&extracted, "1.1.2").is_ok());
        assert!(validate_bundle(&extracted, "1.1.3").is_err());
    }
    #[test]
    fn archive_rejects_special_entries_and_oversized_metadata() {
        for kind in [b'3', b'4', b'6', b'S'] {
            let temp = Temp::new();
            assert!(
                extract_fixture(&temp.0, &archive(&[("Harmony Work.app/special", kind, "")]))
                    .is_err()
            );
        }
        let temp = Temp::new();
        let long_name = "a".repeat(MAX_EXTENSION_BYTES as usize + 1);
        assert!(
            extract_fixture(&temp.0, &archive(&[("././@LongLink", b'L', &long_name)])).is_err()
        );
    }
    #[test]
    fn archive_bounds_entries_file_and_expanded_bytes() {
        for limits in [
            Limits {
                entries: 1,
                ..Limits::default()
            },
            Limits {
                file_bytes: 2,
                ..Limits::default()
            },
            Limits {
                expanded_bytes: 5,
                ..Limits::default()
            },
        ] {
            let temp = Temp::new();
            let payload = temp.0.join("payload.app.tar.gz");
            fs::write(
                &payload,
                archive(&[
                    ("Harmony Work.app/a", b'0', "1234"),
                    ("Harmony Work.app/b", b'0', "5678"),
                ]),
            )
            .unwrap();
            assert!(extract_archive(&payload, &temp.0.join("staged"), limits).is_err());
        }
    }
    #[test]
    fn plist_requires_identity_version_and_real_executable() {
        let temp = Temp::new();
        let bundle = minimal_bundle(&temp.0, "1.1.2");
        assert!(validate_bundle(&bundle, "1.1.2").is_ok());
        assert!(validate_bundle(&bundle, "1.1.3").is_err());
        fs::remove_file(bundle.join(EXECUTABLE_REL)).unwrap();
        std::os::unix::fs::symlink("/bin/sh", bundle.join(EXECUTABLE_REL)).unwrap();
        assert!(validate_bundle(&bundle, "1.1.2").is_err());
    }
    #[test]
    fn plist_rejects_other_product_identifier() {
        let temp = Temp::new();
        let bundle = minimal_bundle(&temp.0, "1.1.2");
        let plist = bundle.join("Contents/Info.plist");
        let text = fs::read_to_string(&plist)
            .unwrap()
            .replace(BUNDLE_ID, "com.other.product");
        fs::write(plist, text).unwrap();
        assert!(validate_bundle(&bundle, "1.1.2").is_err());
    }
    #[test]
    fn manifest_and_health_json_are_bounded_and_reject_symlink_files() {
        let temp = Temp::new();
        let path = temp.0.join(MANIFEST);
        fs::write(&path, vec![b' '; MAX_JSON_BYTES as usize + 1]).unwrap();
        assert!(read_json::<Manifest>(&path).is_err());
        fs::remove_file(&path).unwrap();
        let real = temp.0.join("real.json");
        fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(read_json::<serde_json::Value>(&path).is_err());
    }
    #[test]
    fn transaction_requires_private_canonical_sibling_and_own_helper() {
        let temp = Temp::new();
        let app = minimal_bundle(&temp.0, "1.1.1");
        let tx = temp
            .0
            .join(format!(".harmony-update-{}", uuid::Uuid::new_v4().simple()));
        private_dir(&tx).unwrap();
        let helper = tx.join(HELPER);
        fs::write(&helper, "helper").unwrap();
        let manifest = Manifest::new(app.clone(), "1.1.1", "1.1.2", "0".repeat(64));
        assert!(validate_manifest(&tx, &manifest, Some(&helper)).is_ok());
        assert!(validate_manifest(&tx, &manifest, Some(&app.join(EXECUTABLE_REL))).is_err());
        let mut bad = manifest.clone();
        bad.original = temp.0.parent().unwrap().join(APP_NAME);
        assert!(validate_manifest(&tx, &bad, Some(&helper)).is_err());
        std::os::unix::fs::symlink(&tx, temp.0.join("alias")).unwrap();
        assert!(validate_manifest(&temp.0.join("alias"), &manifest, Some(&helper)).is_err());
        fs::set_permissions(&tx, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(validate_manifest(&tx, &manifest, Some(&helper)).is_err());
    }
    #[test]
    fn health_requires_matching_sibling_manifest_token_and_version() {
        let temp = Temp::new();
        let app = minimal_bundle(&temp.0, "1.1.2");
        let tx = temp
            .0
            .join(format!(".harmony-update-{}", uuid::Uuid::new_v4().simple()));
        private_dir(&tx).unwrap();
        let manifest = Manifest::new(app.clone(), "1.1.1", "1.1.2", "0".repeat(64));
        write_manifest(&tx, &manifest).unwrap();
        let exe = app.join(EXECUTABLE_REL);
        assert!(valid_health_target(
            &exe,
            &tx.join(HEALTH),
            &manifest.token,
            "1.1.2"
        ));
        assert!(!valid_health_target(
            &exe,
            &tx.join(HEALTH),
            &"a".repeat(32),
            "1.1.2"
        ));
        assert!(!valid_health_target(
            &exe,
            &tx.join(HEALTH),
            &manifest.token,
            "1.1.3"
        ));
        assert!(!valid_health_target(
            &exe,
            &tx.join("other.json"),
            &manifest.token,
            "1.1.2"
        ));
    }
    #[test]
    fn install_swap_and_health_failure_restore_original_without_deleting_evidence() {
        let temp = Temp::new();
        let original = minimal_bundle(&temp.0, "1.1.1");
        let tx = temp.0.join("transaction");
        private_dir(&tx).unwrap();
        let staged_root = tx.join("staged");
        private_dir(&staged_root).unwrap();
        let staged = minimal_bundle(&staged_root, "1.1.2");
        let backup = tx.join(BACKUP);
        swap_bundle(&original, &staged, &backup).unwrap();
        assert!(validate_bundle(&original, "1.1.2").is_ok());
        assert!(validate_bundle(&backup, "1.1.1").is_ok());
        rollback_bundle(&original, &backup, &tx.join(FAILED)).unwrap();
        assert!(validate_bundle(&original, "1.1.1").is_ok());
        assert!(validate_bundle(&tx.join(FAILED), "1.1.2").is_ok());
    }
    #[test]
    fn failed_second_rename_restores_old_bundle() {
        let temp = Temp::new();
        let original = minimal_bundle(&temp.0, "1.1.1");
        let stage_root = temp.0.join("staged");
        private_dir(&stage_root).unwrap();
        let staged = minimal_bundle(&stage_root, "1.1.2");
        let mut calls = 0;
        let result = swap_with(&original, &staged, &temp.0.join(BACKUP), |from, to| {
            calls += 1;
            if calls == 2 {
                Err(io::Error::other("injected swap failure"))
            } else {
                fs::rename(from, to)
            }
        });
        assert!(result.is_err());
        assert!(validate_bundle(&original, "1.1.1").is_ok());
        assert!(validate_bundle(&staged, "1.1.2").is_ok());
    }
    #[test]
    fn swap_never_overwrites_an_existing_backup_even_a_dangling_link() {
        let temp = Temp::new();
        let original = minimal_bundle(&temp.0, "1.1.1");
        let stage_root = temp.0.join("staged");
        private_dir(&stage_root).unwrap();
        let staged = minimal_bundle(&stage_root, "1.1.2");
        let backup = temp.0.join(BACKUP);
        std::os::unix::fs::symlink(temp.0.join("missing"), &backup).unwrap();
        assert!(swap_bundle(&original, &staged, &backup).is_err());
        assert!(validate_bundle(&original, "1.1.1").is_ok());
        assert!(fs::symlink_metadata(&backup)
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
