//! Native updater. Only signed, freshly checked payloads reach installer handoff.
mod policy;
pub use policy::{admit_work, WorkPermit};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    version: String,
    notes: String,
    date: Option<String>,
    download_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshot {
    phase: String,
    current_version: String,
    channel: String,
    available: Option<Available>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<String>,
    next_check_at: Option<u64>,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Saved {
    channel: String,
    next_check_at: Option<u64>,
    last_success: Option<u64>,
    failures: u32,
    available: Option<Available>,
}
impl Saved {
    fn clear_obsolete_available(&mut self, current: &semver::Version) {
        if self.available.as_ref().is_some_and(|available| {
            semver::Version::parse(&available.version)
                .map(|version| version <= *current)
                .unwrap_or(true)
        }) {
            self.available = None;
        }
    }
    fn record_failure(&mut self, at: u64) {
        self.failures = self.failures.saturating_add(1);
        self.next_check_at = Some(policy::failure_due(at, self.failures));
    }
}
struct Inner {
    snapshot: UpdateSnapshot,
    saved: Saved,
    completed: u64,
    last_error: Option<String>,
    cancel: CancellationToken,
}
pub struct UpdateService {
    inner: Mutex<Inner>,
    transaction: Arc<tokio::sync::Mutex<()>>,
    path: PathBuf,
    owner: Option<std::fs::File>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn jitter(max: u64) -> u64 {
    let mut bytes = [0; 8];
    let _ = getrandom::getrandom(&mut bytes);
    u64::from_le_bytes(bytes) % (max + 1)
}
impl UpdateService {
    fn refresh_shared(&self, app: &tauri::AppHandle) {
        if self.owner.is_some() {
            return;
        }
        let Some(saved) = std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
        else {
            return;
        };
        if !policy::valid_channel(&saved.channel) {
            return;
        }
        self.change(app, |s| {
            s.channel = saved.channel;
            s.available = saved.available;
            s.next_check_at = saved.next_check_at;
            s.phase = if s.available.is_some() {
                "available"
            } else {
                "idle"
            }
            .into();
        });
    }
    fn snapshot(&self) -> UpdateSnapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }
    fn change(&self, app: &tauri::AppHandle, f: impl FnOnce(&mut UpdateSnapshot)) {
        let snapshot = {
            let mut inner = self.inner.lock().unwrap();
            f(&mut inner.snapshot);
            inner.snapshot.clone()
        };
        let _ = app.emit("harmony-update-state", snapshot);
    }
    fn persist(&self) {
        if self.owner.is_none() {
            return;
        }
        let inner = self.inner.lock().unwrap();
        if let Ok(bytes) = serde_json::to_vec_pretty(&inner.saved) {
            let temp = self.path.with_extension("tmp");
            if std::fs::write(&temp, bytes).is_ok() {
                if let Err(error) = replace_state_file(&temp, &self.path) {
                    tracing::warn!("Updater schedule save failed: {error}");
                }
            }
        }
    }
}
fn replace_state_file(temp: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::rename(temp, target)
}
fn main_only(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        Err("更新操作仅限主窗口".into())
    } else {
        Ok(())
    }
}
fn supported(service: &UpdateService) -> Result<(), String> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err("此平台暂不支持客户端更新".into());
    }
    if service.owner.is_none() {
        return Err("另一个客户端实例正在管理更新".into());
    }
    Ok(())
}
fn endpoint(channel: &str) -> String {
    format!("https://download.harmonywatch.cn/updates/windows/x86_64/{channel}.json")
}
async fn fresh_update(
    app: &tauri::AppHandle,
    channel: &str,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint(channel)
            .parse()
            .map_err(|e| format!("{e}"))?])
        .map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(15))
        .configure_client(|client| client.redirect(updater_reqwest::redirect::Policy::none()))
        .build()
        .map_err(|e| e.to_string())?;
    let update = tokio::time::timeout(Duration::from_secs(15), updater.check())
        .await
        .map_err(|_| "检查更新超时，请重试".to_string())?
        .map_err(|e| e.to_string())?;
    if let Some(update) = &update {
        if !policy::allowed_download(update.download_url.as_str()) {
            return Err("更新包地址不属于可信发布目录".into());
        }
        if channel == "stable" && update.version.contains('-') {
            return Err("稳定频道拒绝预览版本".into());
        }
        if update.signature.trim().is_empty() {
            return Err("更新包缺少签名".into());
        }
    }
    Ok(update)
}
fn available(update: &tauri_plugin_updater::Update) -> Available {
    Available {
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        date: update.date.map(|date| date.to_string()),
        download_url: update.download_url.to_string(),
    }
}
async fn check(
    app: &tauri::AppHandle,
    service: &UpdateService,
    manual: bool,
) -> Result<UpdateSnapshot, String> {
    supported(service)?;
    let _transaction = service.transaction.lock().await;
    let channel = {
        let mut inner = service.inner.lock().unwrap();
        if now().saturating_sub(inner.completed) < 10
            || (!manual && inner.snapshot.next_check_at.is_some_and(|due| due > now()))
        {
            if manual && inner.last_error.is_some() {
                inner.snapshot.error = inner.last_error.clone();
                inner.snapshot.phase = if inner.snapshot.available.is_some() {
                    "available"
                } else {
                    "error"
                }
                .into();
            }
            let snapshot = inner.snapshot.clone();
            drop(inner);
            let _ = app.emit("harmony-update-state", snapshot.clone());
            return Ok(snapshot);
        }
        inner.snapshot.channel.clone()
    };
    service.change(app, |s| {
        s.phase = "checking".into();
        s.error = None;
    });
    let result = fresh_update(app, &channel).await;
    {
        let mut inner = service.inner.lock().unwrap();
        inner.completed = now();
        inner.last_error = result.as_ref().err().cloned();
        match &result {
            Ok(update) => {
                inner.saved.available = update.as_ref().map(available);
                inner.saved.failures = 0;
                inner.saved.last_success = Some(now());
                inner.saved.next_check_at = Some(policy::success_due(now(), jitter(900)));
                inner.snapshot.available = inner.saved.available.clone();
            }
            Err(_) => {
                inner.saved.record_failure(now());
            }
        }
        inner.snapshot.next_check_at = inner.saved.next_check_at;
    }
    service.persist();
    service.change(app, |s| match result {
        Ok(_) => {
            s.phase = if s.available.is_some() {
                "available"
            } else {
                "current"
            }
            .into();
            s.error = None;
        }
        Err(error) => {
            tracing::warn!("Updater check failed: {error}");
            s.phase = if s.available.is_some() {
                "available"
            } else if manual {
                "error"
            } else {
                "idle"
            }
            .into();
            s.error = if manual { Some(error) } else { None };
        }
    });
    Ok(service.snapshot())
}
#[tauri::command]
pub fn update_status(
    window: tauri::WebviewWindow,
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<UpdateSnapshot, String> {
    main_only(&window)?;
    service.refresh_shared(window.app_handle());
    acknowledge_health(window.app_handle());
    Ok(service.snapshot())
}
#[tauri::command]
pub async fn update_check(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<UpdateService>>,
    manual: bool,
) -> Result<UpdateSnapshot, String> {
    main_only(&window)?;
    check(&app, &service, manual).await
}
#[tauri::command]
pub fn update_cancel(
    window: tauri::WebviewWindow,
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<UpdateSnapshot, String> {
    main_only(&window)?;
    let inner = service.inner.lock().unwrap();
    if inner.snapshot.phase == "installing" {
        return Err("安装器已接管，无法取消".into());
    }
    inner.cancel.cancel();
    Ok(inner.snapshot.clone())
}
#[tauri::command]
pub async fn update_set_channel(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<UpdateService>>,
    channel: String,
) -> Result<UpdateSnapshot, String> {
    main_only(&window)?;
    supported(&service)?;
    if !policy::valid_channel(&channel) {
        return Err("未知更新频道".into());
    }
    let _transaction = service
        .transaction
        .try_lock()
        .map_err(|_| "更新事务正在运行")?;
    {
        let mut inner = service.inner.lock().unwrap();
        inner.saved = Saved {
            channel: channel.clone(),
            next_check_at: Some(now()),
            ..Default::default()
        };
        inner.completed = 0;
        inner.last_error = None;
    }
    service.change(&app, |s| {
        s.channel = channel;
        s.available = None;
        s.error = None;
        s.phase = "idle".into();
        s.next_check_at = Some(now());
    });
    service.persist();
    Ok(service.snapshot())
}
#[tauri::command]
pub async fn update_install(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    service: tauri::State<'_, Arc<UpdateService>>,
) -> Result<UpdateSnapshot, String> {
    main_only(&window)?;
    supported(&service)?;
    // Reserve the transaction before returning, preventing duplicate button presses.
    let owned = service.inner().clone();
    let transaction = owned
        .transaction
        .clone()
        .try_lock_owned()
        .map_err(|_| "更新事务正在运行")?;
    {
        let mut inner = owned.inner.lock().unwrap();
        inner.cancel = CancellationToken::new();
    }
    service.change(&app, |s| {
        s.phase = "checking".into();
        s.error = None;
        s.downloaded = 0;
        s.total = None;
    });
    tauri::async_runtime::spawn(async move {
        let _transaction = transaction;
        let cancel = owned.inner.lock().unwrap().cancel.clone();
        let result = install(&app, &owned, &cancel).await;
        match result {
            Err(error) => owned.change(&app, |s| {
                s.phase = "error".into();
                s.error = Some(error);
            }),
            Ok(()) => {
                if owned.snapshot().phase != "installing" {
                    owned.change(&app, |s| {
                        s.phase = if s.available.is_some() {
                            "available"
                        } else {
                            "idle"
                        }
                        .into();
                        s.downloaded = 0;
                        s.total = None;
                    });
                }
            }
        }
    });
    Ok(service.snapshot())
}
fn installer_directory_arg(command: &mut std::process::Command, directory: &std::path::Path) {
    // NSIS consumes the remainder after /D=, so it must be last and unquoted.
    // Windows Command::arg would quote the entire argument when spaces occur.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(format!("/D={}", directory.display()));
    }
    #[cfg(not(windows))]
    command.arg(format!("/D={}", directory.display()));
}
async fn install(
    app: &tauri::AppHandle,
    service: &UpdateService,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let mut update = match fresh_update(app, &service.snapshot().channel).await? {
        Some(update) => update,
        None => {
            service.inner.lock().unwrap().saved.available = None;
            service.persist();
            service.change(app, |s| {
                s.available = None;
                s.phase = "current".into();
            });
            return Err("此更新已撤回或当前已是最新版本".into());
        }
    };
    service.change(app, |s| {
        s.available = Some(available(&update));
        s.phase = "downloading".into();
    });
    update.timeout = Some(Duration::from_secs(1800));
    let download = update.download(
        |size, total| {
            service.change(app, |s| {
                s.downloaded += size as u64;
                s.total = total;
            })
        },
        || service.change(app, |s| s.phase = "verifying".into()),
    );
    let bytes = tokio::select! { _=cancel.cancelled()=>return Ok(()), result=download=>result.map_err(|e|e.to_string())? };
    // Update.download returns bytes only AFTER minisign verification succeeds.
    if cancel.is_cancelled() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = exe.parent().ok_or("无法确定安装目录")?;
    if !directory.join("uninstall.exe").is_file() {
        return Err("请先使用完整安装包安装 Harmony Work，再应用轻量更新".into());
    }
    let stage = service
        .path
        .parent()
        .ok_or("更新目录不可用")?
        .join(format!("payload-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let installer = stage.join("harmony-work-update.exe");
    std::fs::write(&installer, &bytes).map_err(|e| e.to_string())?;
    use sha2::Digest;
    std::fs::write(
        stage.join("payload.sha256"),
        hex::encode(sha2::Sha256::digest(&bytes)),
    )
    .map_err(|e| e.to_string())?;
    // The verified installer owns the compatibility schema. Its dry run hashes
    // baseline dependencies and checks writable path / free space before exit.
    let mut preflight = tokio::process::Command::new(&installer);
    preflight.args(["/S", "/CHECK"]).kill_on_drop(true);
    installer_directory_arg(preflight.as_std_mut(), directory);
    let status = tokio::select! {
        _=cancel.cancelled()=>{let _=std::fs::remove_dir_all(&stage);return Ok(());},
        result=tokio::time::timeout(Duration::from_secs(120),preflight.status())=>result.map_err(|_|"更新兼容性检查超时")?.map_err(|e|format!("无法检查更新兼容性: {e}"))?
    };
    if !status.success() {
        return Err("轻量更新兼容性检查失败，请使用完整安装包；详见临时目录 HarmonyWorkUpdateCheck/light-installer.log".into());
    }
    let _quiesce = policy::Quiesce::begin();
    service.change(app, |s| s.phase = "waiting".into());
    while !_quiesce.ready()
        || if let Some(state) = app.try_state::<crate::store::AppState>() {
            !state.cancel_flags.lock().await.is_empty()
        } else {
            false
        }
    {
        tokio::select! { _=cancel.cancelled()=>{let _=std::fs::remove_dir_all(&stage);return Ok(());}, _=tokio::time::sleep(Duration::from_millis(250))=>{} }
    }
    if cancel.is_cancelled() {
        let _ = std::fs::remove_dir_all(&stage);
        return Ok(());
    }
    // No cancellation once cleanup/handoff begins.
    service.change(app, |s| s.phase = "installing".into());
    if let Some(runtime) = app.try_state::<crate::codex_runtime::service::CodexRuntimeState>() {
        runtime.shutdown().await.map_err(|e| e.to_string())?;
    }
    if let Some(state) = app.try_state::<crate::store::AppState>() {
        crate::app::shutdown::prepare_shutdown(&state).await;
    }
    let mut command = std::process::Command::new(&installer);
    command.args(["/S", "/UPDATE", "/R"]);
    installer_directory_arg(&mut command, directory);
    command
        .spawn()
        .map_err(|e| format!("无法启动更新安装器: {e}"))?;
    _quiesce.commit();
    crate::app::shutdown::request_app_exit(app.clone());
    Ok(())
}
pub fn setup(app: &tauri::AppHandle) {
    let directory = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("harmony-work"))
        .join("updater");
    if let Err(error) = std::fs::create_dir_all(&directory) {
        tracing::warn!("Updater storage unavailable: {error}");
    }
    let path = directory.join("state.json");
    let mut saved: Saved = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if !policy::valid_channel(&saved.channel) {
        saved.channel = "stable".into();
        saved.available = None;
    }
    saved.clear_obsolete_available(&app.package_info().version);
    if saved.next_check_at.is_none() {
        saved.next_check_at = Some(now() + 30 + jitter(30));
    }
    let owner = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("owner.lock"))
        .ok()
        .filter(|file| fs2::FileExt::try_lock_exclusive(file).is_ok());
    let snapshot = UpdateSnapshot {
        phase: if saved.available.is_some() {
            "available"
        } else {
            "idle"
        }
        .into(),
        current_version: app.package_info().version.to_string(),
        channel: saved.channel.clone(),
        available: saved.available.clone(),
        downloaded: 0,
        total: None,
        error: None,
        next_check_at: saved.next_check_at,
    };
    let service = Arc::new(UpdateService {
        inner: Mutex::new(Inner {
            snapshot,
            saved,
            completed: 0,
            last_error: None,
            cancel: CancellationToken::new(),
        }),
        transaction: Arc::new(tokio::sync::Mutex::new(())),
        path,
        owner,
    });
    service.persist();
    app.manage(service.clone());
    let handle = app.clone();
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if service.owner.is_some() {
                    let _ = check(&handle, &service, false).await;
                } else {
                    service.refresh_shared(&handle);
                }
            }
        });
    }
}

fn write_atomic_health(target: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "health target lacks parent",
        )
    })?;
    let temporary = parent.join(format!(".health-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        // Same-directory replacement exposes either the complete old or new JSON.
        std::fs::rename(&temporary, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
fn valid_health_target(install: &std::path::Path, target: &std::path::Path) -> bool {
    if !target.is_absolute() || target.file_name().and_then(|s| s.to_str()) != Some("health.json") {
        return false;
    }
    let Some(parent) = target.parent() else {
        return false;
    };
    let Some(name) = parent
        .file_name()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix(".harmony-update-"))
    else {
        return false;
    };
    if name.len() != 32 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let Ok(parent) = parent.canonicalize() else {
        return false;
    };
    let Ok(install) = install.canonicalize() else {
        return false;
    };
    parent.parent() == Some(install.as_path())
}
fn acknowledge_health(app: &tauri::AppHandle) {
    // Called from main-window IPC after the frontend initializes. Installer
    // environment is accepted only for its existing transaction under install.
    if app.get_webview_window("main").is_none() {
        return;
    }
    if let (Ok(file), Ok(token), Ok(exe)) = (
        std::env::var("HARMONY_UPDATE_HEALTH_FILE"),
        std::env::var("HARMONY_UPDATE_HEALTH_TOKEN"),
        std::env::current_exe(),
    ) {
        if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return;
        }
        let target = std::path::Path::new(&file);
        if !exe
            .parent()
            .is_some_and(|install| valid_health_target(install, target))
        {
            return;
        }
        let bytes =
            serde_json::json!({"token":token,"version":app.package_info().version.to_string()})
                .to_string();
        if let Err(error) = write_atomic_health(target, bytes.as_bytes()) {
            tracing::warn!("Updater health acknowledgement failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_and_available_survive_disk_roundtrip() {
        let saved = Saved {
            channel: "preview".into(),
            next_check_at: Some(123456),
            last_success: Some(100),
            failures: 2,
            available: Some(Available {
                version: "1.1.2-rc.1".into(),
                notes: "notes".into(),
                date: None,
                download_url: "https://download.harmonywatch.cn/releases/test.exe".into(),
            }),
        };
        let restored: Saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(restored.next_check_at, Some(123456));
        assert_eq!(restored.last_success, Some(100));
        assert_eq!(restored.available.unwrap().version, "1.1.2-rc.1");
    }
    #[tokio::test]
    async fn transaction_is_single_flight_and_guard_releases_after_cancel() {
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        let guard = lock.clone().try_lock_owned().unwrap();
        assert!(lock.try_lock().is_err());
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        task.abort();
        let _ = task.await;
        assert!(lock.try_lock().is_ok());
    }
    #[test]
    fn failed_check_preserves_confirmed_available() {
        let mut saved = Saved {
            available: Some(Available {
                version: "1.2.0".into(),
                notes: "confirmed".into(),
                date: None,
                download_url: "https://download.harmonywatch.cn/releases/update.exe".into(),
            }),
            ..Default::default()
        };
        saved.record_failure(10);
        assert_eq!(saved.available.unwrap().version, "1.2.0");
        assert_eq!(saved.next_check_at, Some(310));
    }
    #[test]
    fn health_target_is_confined_to_installer_transaction() {
        let root =
            std::env::temp_dir().join(format!("harmony-health-target-{}", uuid::Uuid::new_v4()));
        let transaction = root.join(format!(".harmony-update-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&transaction).unwrap();
        assert!(valid_health_target(&root, &transaction.join("health.json")));
        assert!(!valid_health_target(
            &root,
            &transaction.join("settings.json")
        ));
        assert!(!valid_health_target(
            &transaction,
            &transaction.join("health.json")
        ));
        assert!(!valid_health_target(
            &root,
            std::path::Path::new("relative/health.json")
        ));
        assert!(!valid_health_target(&root, &root.join("health.json")));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn health_ack_is_atomic_for_concurrent_readers() {
        let dir =
            std::env::temp_dir().join(format!("harmony-health-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let target = dir.join("health.json");
        let bytes =
            serde_json::to_vec(&serde_json::json!({"token":"first","version":"1.1.1"})).unwrap();
        write_atomic_health(&target, &bytes).unwrap();
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let reader_flag = running.clone();
        let reader_target = target.clone();
        let reader = std::thread::spawn(move || {
            let mut reads = 0;
            while reader_flag.load(std::sync::atomic::Ordering::SeqCst) {
                let data = std::fs::read(&reader_target).unwrap();
                let json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(json["version"], "1.1.1");
                reads += 1;
            }
            reads
        });
        for i in 0..100 {
            write_atomic_health(
                &target,
                &serde_json::to_vec(&serde_json::json!({"token":i,"version":"1.1.1"})).unwrap(),
            )
            .unwrap();
        }
        running.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(reader.join().unwrap() > 0);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(write_atomic_health(&dir.join("missing/health.json"), &bytes).is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn startup_clears_installed_older_and_invalid_cached_updates() {
        let current = semver::Version::parse("1.1.2-rc.1").unwrap();
        for (version, retained) in [
            ("1.1.2-rc.1", false),
            ("1.1.1", false),
            ("invalid", false),
            ("1.1.2", true),
            ("1.1.2-rc.2", true),
        ] {
            let mut saved = Saved {
                available: Some(Available {
                    version: version.into(),
                    notes: String::new(),
                    date: None,
                    download_url: String::new(),
                }),
                next_check_at: Some(123456),
                ..Default::default()
            };
            saved.clear_obsolete_available(&current);
            assert_eq!(saved.available.is_some(), retained, "{version}");
            assert_eq!(saved.next_check_at, Some(123456));
        }
    }
    #[test]
    fn ipc_snapshot_is_camel_case() {
        let json = serde_json::to_value(UpdateSnapshot {
            phase: "idle".into(),
            current_version: "1.1.1".into(),
            channel: "stable".into(),
            available: None,
            downloaded: 0,
            total: None,
            error: None,
            next_check_at: Some(42),
        })
        .unwrap();
        assert_eq!(json["currentVersion"], "1.1.1");
        assert_eq!(json["nextCheckAt"], 42);
        assert!(json.get("current_version").is_none());
    }
}
