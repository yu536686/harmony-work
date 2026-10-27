use crate::skills::loader::check_skill_compatibility;
use crate::store::{db::Skill, AppState};
use serde::Serialize;
use futures::StreamExt;
use tauri::{Emitter, Manager, State};
use tracing::{info, warn};

// reqwest is available as a transitive dependency from other tools
use reqwest;

fn notify_skills_changed(state: &State<'_, AppState>) {
    let _ = state.app_handle.emit("skills_changed", ());
}

/// Perform a GET request with automatic retry on 429 (rate-limit) and 5xx errors.
///
/// Retry schedule: up to `max_retries` attempts with exponential back-off starting at
/// `base_delay_ms` ms (doubled each attempt, capped at 16 s).
/// Respects the `Retry-After` header when present.
async fn clawhub_get_with_retry(
    client: &reqwest::Client,
    url: &str,
    max_retries: u32,
) -> Result<reqwest::Response, String> {
    let url = crate::service_urls::migrate_public_service_url(url);
    let base_delay_ms: u64 = 1000;
    let mut attempt = 0u32;

    loop {
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("网络请求失败：{}", e))?;

        let status = resp.status();

        // Success or a client error that won't be fixed by retrying (4xx except 429)
        if status.is_success() || (status.is_client_error() && status.as_u16() != 429) {
            return Ok(resp);
        }

        // 429 or 5xx — potentially retryable
        if attempt >= max_retries {
            return Ok(resp); // return the last response; caller handles the error status
        }

        // Honour Retry-After header if present (value in seconds)
        let retry_after_ms = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|secs| secs * 1000)
            .unwrap_or(0);

        let backoff_ms = if retry_after_ms > 0 {
            retry_after_ms.min(30_000) // cap at 30 s
        } else {
            let exp = base_delay_ms * (1u64 << attempt.min(4)); // 1s, 2s, 4s, 8s, 16s
            exp.min(16_000)
        };

        warn!(
            "ClawHub {} for '{}', retrying in {}ms (attempt {}/{})",
            status,
            url,
            backoff_ms,
            attempt + 1,
            max_retries
        );
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        attempt += 1;
    }
}

#[derive(Debug, Serialize)]
pub struct SkillList {
    pub skills: Vec<Skill>,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillRuntimeState {
    Ready,
    RepairRequired,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRuntimeVerification {
    pub id: String,
    pub installed: bool,
    pub enabled: bool,
    pub dependencies_ready: bool,
    pub runtime_state: SkillRuntimeState,
    pub repair_hint: Option<String>,
    pub missing_dependencies: Vec<String>,
}

fn mcp_dependency_name(dependency: &str) -> Option<String> {
    let value = dependency.trim();
    let name = value
        .strip_prefix("mcp:")
        .or_else(|| value.strip_prefix("mcp/"))?;
    let name = name.split('/').next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_lowercase())
}

fn local_command_dependency_name(dependency: &str) -> Option<String> {
    let raw = dependency.to_lowercase();
    let executable = raw
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
        .next()
        .unwrap_or_default();
    let executable = match executable {
        "python3" => "python",
        "nodejs" => "node",
        "dotnet-sdk" | "dotnet-runtime" => "dotnet",
        other => other,
    };
    const KNOWN_RUNTIMES: &[&str] = &[
        "python", "node", "npm", "npx", "ruby", "go", "java", "dotnet", "git", "ffmpeg",
        "cargo", "pip", "pip3",
    ];
    KNOWN_RUNTIMES
        .contains(&executable)
        .then(|| executable.to_string())
}

fn command_is_available(command: &str) -> bool {
    let finder = if cfg!(windows) { "where" } else { "which" };
    crate::process::std_command(finder)
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn verify_skill_runtime_from<F>(
    skill_id: &str,
    record: Option<&Skill>,
    skills_root: &std::path::Path,
    enabled_mcp_servers: &std::collections::HashSet<String>,
    command_available: F,
) -> SkillRuntimeVerification
where
    F: Fn(&str) -> bool,
{
    let enabled = record.is_some_and(|skill| skill.enabled);
    let Some(skill_file) = crate::skills::provenance::find_skill_md(skills_root, skill_id) else {
        return SkillRuntimeVerification {
            id: skill_id.to_string(),
            installed: false,
            enabled,
            dependencies_ready: false,
            runtime_state: SkillRuntimeState::RepairRequired,
            repair_hint: Some("未找到已安装技能的 SKILL.md，请同步磁盘或重新安装。".into()),
            missing_dependencies: vec!["SKILL.md".into()],
        };
    };

    let content = match std::fs::read_to_string(&skill_file) {
        Ok(content) => content,
        Err(error) => {
            return SkillRuntimeVerification {
                id: skill_id.to_string(),
                installed: false,
                enabled,
                dependencies_ready: false,
                runtime_state: SkillRuntimeState::RepairRequired,
                repair_hint: Some(format!("无法读取 SKILL.md：{error}")),
                missing_dependencies: vec!["SKILL.md".into()],
            };
        }
    };
    let loader = crate::skills::loader::SkillLoader::new(skills_root);
    let definition = match loader.parse_skill_from_content(&content) {
        Ok(definition) => definition,
        Err(error) => {
            return SkillRuntimeVerification {
                id: skill_id.to_string(),
                installed: false,
                enabled,
                dependencies_ready: false,
                runtime_state: SkillRuntimeState::RepairRequired,
                repair_hint: Some(format!("SKILL.md 无法解析：{error}")),
                missing_dependencies: vec!["SKILL.md".into()],
            };
        }
    };

    let installed = record.is_some();
    let mut missing_dependencies = Vec::new();
    for dependency in &definition.dependencies {
        if let Some(mcp_name) = mcp_dependency_name(dependency) {
            if !enabled_mcp_servers.contains(&mcp_name) {
                missing_dependencies.push(format!("mcp:{mcp_name}"));
            }
        } else if let Some(command) = local_command_dependency_name(dependency) {
            if !command_available(&command) {
                missing_dependencies.push(command);
            }
        }
    }

    let dependencies_ready = installed && missing_dependencies.is_empty();
    let repair_hint = if !installed {
        Some("技能元数据尚未登记，请从磁盘同步后再验证。".into())
    } else if !enabled {
        Some("技能已安装但未启用；启用后再验证即可调用。".into())
    } else if !dependencies_ready {
        Some(format!(
            "缺少运行时依赖：{}。修复后重新验证。",
            missing_dependencies.join("、")
        ))
    } else {
        None
    };
    let runtime_state = if installed && enabled && dependencies_ready {
        SkillRuntimeState::Ready
    } else {
        SkillRuntimeState::RepairRequired
    };

    SkillRuntimeVerification {
        id: skill_id.to_string(),
        installed,
        enabled,
        dependencies_ready,
        runtime_state,
        repair_hint,
        missing_dependencies,
    }
}

/// Read-only runtime verification. This only reads installed metadata and checks
/// command/MCP discovery; it never launches a Skill's user business command.
#[tauri::command]
pub async fn verify_skill_runtime(
    state: State<'_, AppState>,
    id: String,
) -> Result<SkillRuntimeVerification, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_root = crate::skills::service::skills_root_from_app_data(&app_dir);
    let record = {
        let db = state.db.lock().await;
        db.get_skill(&id).map_err(|error| error.to_string())?
    };
    let enabled_mcp_servers = {
        let settings = state.settings.lock().await;
        settings
            .mcp_servers
            .iter()
            .filter(|server| server.enabled)
            .map(|server| server.name.to_lowercase())
            .collect::<std::collections::HashSet<_>>()
    };

    Ok(verify_skill_runtime_from(
        &id,
        record.as_ref(),
        &skills_root,
        &enabled_mcp_servers,
        command_is_available,
    ))
}

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<SkillList, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_dir = crate::skills::service::skills_root_from_app_data(&app_dir);
    let mut loader = crate::skills::loader::SkillLoader::new(&skills_dir);
    loader.load_all().map_err(|e| e.to_string())?;
    let fs_skill_ids: std::collections::HashSet<String> = loader
        .list_skills()
        .into_iter()
        .map(|s| s.skill_id.clone())
        .collect();

    let db = state.db.lock().await;
    let skills = db.list_skills().map_err(|e| e.to_string())?;
    // Filter out any stale "unnamed" entries left by failed skill parses
    let skills: Vec<_> = skills
        .into_iter()
        .filter(|s| s.name != "unnamed" && s.id != "unnamed" && fs_skill_ids.contains(&s.id))
        .collect();
    let total = skills.len();
    Ok(SkillList { skills, total })
}

#[tauri::command]
pub async fn toggle_skill(
    state: State<'_, AppState>,
    skill_id: String,
    enabled: bool,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.set_skill_enabled(&skill_id, enabled)
        .map_err(|e| e.to_string())
        .map(|_| notify_skills_changed(&state))
}

#[derive(Debug, Serialize)]
pub struct SkillCatalogItem {
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub path: String,
    pub market_id: Option<String>,
    pub market_version: Option<String>,
    pub tools: Vec<String>,
    pub dependencies: Vec<String>,
    pub permissions: Vec<String>,
    pub platform: Vec<String>,
}

fn read_market_metadata(path: &std::path::Path) -> (Option<String>, Option<String>) {
    let Ok(bytes) = std::fs::read(path.join(".harmony-market.json")) else {
        return (None, None);
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return (None, None);
    };
    (
        value["id"].as_str().map(str::to_string),
        value["version"].as_str().map(str::to_string),
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarmonyInstallProgress<'a> {
    slug: &'a str,
    phase: &'a str,
    percent: u8,
}

fn emit_harmony_install_progress(state: &State<'_, AppState>, slug: &str, phase: &str, percent: u8) {
    let _ = state.app_handle.emit(
        "harmony_skill_install_progress",
        HarmonyInstallProgress { slug, phase, percent },
    );
}

#[tauri::command]
pub async fn scan_skill_catalog(
    state: State<'_, AppState>,
) -> Result<Vec<SkillCatalogItem>, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_dir = app_dir.join("skills");
    let mut loader = crate::skills::loader::SkillLoader::new(&skills_dir);
    loader.load_all().map_err(|e| e.to_string())?;

    let fs_skills = loader.list_skills();

    let items = fs_skills
        .into_iter()
        .filter(|s| !s.name.is_empty() && s.name != "unnamed")
        .map(|s| SkillCatalogItem {
            name: s.name.clone(),
            description: s.description.clone(),
            version: s.version.clone(),
            source: s.source.clone(),
            path: s.source_path.to_string_lossy().into_owned(),
            market_id: read_market_metadata(&s.source_path).0,
            market_version: read_market_metadata(&s.source_path).1,
            tools: s.tools.clone(),
            dependencies: s.dependencies.clone(),
            permissions: s.permissions.clone(),
            platform: s.platform.clone(),
        })
        .collect::<Vec<_>>();
    Ok(items)
}

/// Scan the skills directory on disk and register any skills that are present
/// on the filesystem but not yet in the database.  Already-registered skills
/// are left untouched (their `enabled` flag is preserved).
///
/// Returns a summary: `{ synced: N, already_registered: M, errors: [...] }`
#[derive(Debug, Serialize)]
pub struct SyncSkillsResult {
    pub synced: usize,
    pub already_registered: usize,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn sync_skills_from_disk(state: State<'_, AppState>) -> Result<SyncSkillsResult, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_dir = app_dir.join("skills");

    // Load all skills from the filesystem
    let mut loader = crate::skills::loader::SkillLoader::new(&skills_dir);
    loader.load_all().map_err(|e| e.to_string())?;
    let fs_skills = loader.list_skills();

    // Get the set of skill IDs already in the DB
    let db_skill_ids: std::collections::HashSet<String> = {
        let db = state.db.lock().await;
        db.list_skills()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.id)
            .collect()
    };

    let mut synced = 0usize;
    let mut already_registered = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for skill in fs_skills {
        if skill.name.is_empty() || skill.name == "unnamed" {
            continue;
        }

        // Derive the same safe_name key used by install_skill
        let safe_name: String = skill
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .to_lowercase();

        if db_skill_ids.contains(&safe_name) {
            already_registered += 1;
            continue;
        }

        let meta = match skill.lifecycle.as_str() {
            crate::skills::provenance::LIFECYCLE_BUILTIN => {
                crate::skills::provenance::SkillConfigMeta::builtin()
            }
            crate::skills::provenance::LIFECYCLE_DRAFT => {
                crate::skills::provenance::SkillConfigMeta::draft("sync", None)
            }
            crate::skills::provenance::LIFECYCLE_LEARNED => {
                let mut m = crate::skills::provenance::SkillConfigMeta::draft("sync", None);
                m.lifecycle = crate::skills::provenance::LIFECYCLE_LEARNED.to_string();
                m
            }
            _ => crate::skills::provenance::SkillConfigMeta::installed("sync", None, None),
        };
        let skill_id = if !skill.skill_id.is_empty() {
            skill.skill_id.clone()
        } else {
            safe_name.clone()
        };

        let db = state.db.lock().await;
        match db.upsert_skill_with_config(
            &skill_id,
            &skill.name,
            &skill.description,
            "📦",
            Some(&meta.to_json()),
        ) {
            Ok(_) => {
                let _ = db.ensure_skill_usage(&skill_id, meta.source.as_deref());
                info!("sync_skills_from_disk: registered '{}'", skill.name);
                synced += 1;
            }
            Err(e) => {
                errors.push(format!("'{}': {}", skill.name, e));
            }
        }
    }

    if synced > 0 {
        notify_skills_changed(&state);
    }
    Ok(SyncSkillsResult {
        synced,
        already_registered,
        errors,
    })
}

/// Install a skill from a URL (raw SKILL.md) or local file path.
/// The SKILL.md is downloaded, parsed, and written to the app skills directory.
async fn install_skill_from_content(
    state: &State<'_, AppState>,
    content: String,
) -> Result<SkillCatalogItem, String> {
    install_skill_from_content_sourced(state, content, "manual", None).await
}

async fn install_skill_from_content_sourced(
    state: &State<'_, AppState>,
    content: String,
    source: &str,
    source_url: Option<String>,
) -> Result<SkillCatalogItem, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_dir = crate::skills::service::skills_root_from_app_data(&app_dir);

    let loader = crate::skills::loader::SkillLoader::new(&skills_dir);
    let skill = loader
        .parse_skill_from_content(&content)
        .map_err(|e| format!("Failed to parse SKILL.md: {}", e))?;

    if skill.name.is_empty() || skill.name == "unnamed" {
        return Err("SKILL.md must declare a 'name' field in frontmatter".into());
    }

    let compat = check_skill_compatibility(&skill).await;
    if !compat.compatible {
        return Err(format!(
            "技能 '{}' 与当前系统不兼容：\n{}",
            skill.name,
            compat.issues.join("\n")
        ));
    }
    for w in &compat.warnings {
        warn!("Skill '{}' compatibility warning: {}", skill.name, w);
    }

    if !skill.permissions.is_empty() {
        warn!(
            "Installing skill '{}' with permissions: {:?}",
            skill.name, skill.permissions
        );
    }

    let safe_name = crate::skills::service::sanitize_skill_id(&skill.name);
    validate_zip_path(&safe_name)?;
    let mut stage = new_skill_stage(&skills_dir)?;
    std::fs::write(stage.dir.join("SKILL.md"), &content).map_err(|e| e.to_string())?;
    stage.content = content;
    let installed = crate::skills::provenance::installed_dir(&skills_dir);
    ensure_plain_directory(&installed)?;
    let skill_dir = installed.join(&safe_name);
    {
        let db = state.db.lock().await;
        commit_content_stage(&stage, &skill_dir, &db, &skills_dir, source, source_url)?;
    }
    let skill_file = skill_dir.join("SKILL.md");
    info!("Installed skill '{}' to {:?}", skill.name, skill_dir);

    // Enrich triggers through the same Codex runtime used by the main session.
    {
        let enrich_state = state.inner().clone();
        let enrich_skill = skill.clone();
        let enrich_file = skill_file.clone();
        tokio::spawn(async move {
            if let Err(error) =
                enrich_triggers_with_codex(&enrich_state, &enrich_skill, &enrich_file).await
            {
                warn!("Trigger enrichment failed (non-fatal): {}", error);
            }
        });
    }

    notify_skills_changed(state);
    Ok(SkillCatalogItem {
        name: skill.name,
        description: skill.description,
        version: skill.version,
        source: "installed".to_string(),
        path: skill_dir.to_string_lossy().into_owned(),
        market_id: None,
        market_version: None,
        tools: skill.tools,
        dependencies: skill.dependencies,
        permissions: skill.permissions,
        platform: skill.platform,
    })
}

/// Strip ASCII whitespace AND Unicode invisible/directional characters that get
/// silently inserted when copying paths from Windows Explorer, browsers, or
/// certain chat UIs (e.g. U+202A LEFT-TO-RIGHT EMBEDDING, U+202C POP, U+200B
/// ZERO WIDTH SPACE, U+FEFF BOM, U+00A0 NO-BREAK SPACE, etc.).
fn sanitize_source(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !matches!(
                *c,
                '\u{200B}' // ZERO WIDTH SPACE
                | '\u{200C}' // ZERO WIDTH NON-JOINER
                | '\u{200D}' // ZERO WIDTH JOINER
                | '\u{200E}' // LEFT-TO-RIGHT MARK
                | '\u{200F}' // RIGHT-TO-LEFT MARK
                | '\u{202A}' // LEFT-TO-RIGHT EMBEDDING
                | '\u{202B}' // RIGHT-TO-LEFT EMBEDDING
                | '\u{202C}' // POP DIRECTIONAL FORMATTING
                | '\u{202D}' // LEFT-TO-RIGHT OVERRIDE
                | '\u{202E}' // RIGHT-TO-LEFT OVERRIDE
                | '\u{2060}' // WORD JOINER
                | '\u{FEFF}' // BOM / ZERO WIDTH NO-BREAK SPACE
                | '\u{00A0}' // NO-BREAK SPACE
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[tauri::command]
pub async fn install_skill(
    state: State<'_, AppState>,
    source: String,
) -> Result<SkillCatalogItem, String> {
    let source_trimmed = crate::service_urls::migrate_public_service_url(&sanitize_source(&source));

    // ── Detect zip: local path ending in .zip, or URL ending in .zip ──────────
    let is_zip_url = (source_trimmed.starts_with("http://")
        || source_trimmed.starts_with("https://"))
        && source_trimmed.to_lowercase().ends_with(".zip");
    let is_zip_local = !source_trimmed.starts_with("http://")
        && !source_trimmed.starts_with("https://")
        && source_trimmed.to_lowercase().ends_with(".zip");

    if is_zip_url || is_zip_local {
        return install_skill_from_zip(&state, &source_trimmed).await;
    }

    let content = if source_trimmed.starts_with("http://") || source_trimmed.starts_with("https://")
    {
        // Basic URL validation — reject internal/private addresses
        let blocked = [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "192.168.",
            "10.",
            "172.",
        ];
        for pat in blocked {
            if source_trimmed.contains(pat) {
                return Err(format!(
                    "Blocked URL: '{}' points to a private/local address",
                    source_trimmed
                ));
            }
        }
        info!("Downloading skill from URL: {}", source_trimmed);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(&source_trimmed)
            .header("User-Agent", "Harmony-Work/1.0")
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "HTTP {} when downloading: {}",
                resp.status(),
                source_trimmed
            ));
        }
        resp.text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?
    } else {
        // Local file path
        tokio::fs::read_to_string(&source_trimmed)
            .await
            .map_err(|e| format!("Failed to read local file '{}': {}", source_trimmed, e))?
    };

    install_skill_from_content(&state, content).await
}

/// Install a skill from a .zip archive (local path or URL).
///
/// The zip must contain a `SKILL.md` at the root or inside a single top-level
/// directory. All other files in the same directory as `SKILL.md` are extracted
/// alongside it (e.g. `reference.md`, `examples.md`, helper scripts).
async fn install_skill_from_zip(
    state: &State<'_, AppState>,
    source: &str,
) -> Result<SkillCatalogItem, String> {
    // ── 1. Fetch zip bytes ────────────────────────────────────────────────────
    let zip_bytes: Vec<u8> = if source.starts_with("http://") || source.starts_with("https://") {
        let blocked = [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "192.168.",
            "10.",
            "172.",
        ];
        for pat in blocked {
            if source.contains(pat) {
                return Err(format!(
                    "Blocked URL: '{}' points to a private/local address",
                    source
                ));
            }
        }
        info!("Downloading skill zip from URL: {}", source);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(source)
            .header("User-Agent", "Harmony-Work/1.0")
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "HTTP {} when downloading zip: {}",
                resp.status(),
                source
            ));
        }
        read_zip_response(resp).await?
    } else {
        use tokio::io::AsyncReadExt;
        let file = tokio::fs::File::open(source)
            .await
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(MAX_SKILL_ZIP_BYTES + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_SKILL_ZIP_BYTES {
            return Err("Skill ZIP exceeds download size limit".into());
        }
        bytes
    };

    install_skill_archive(state, &zip_bytes, "manual", None, None).await
}

/// All ZIP entry points use this path, including market downloads and ClawHub fallback.
async fn install_skill_archive(
    state: &State<'_, AppState>,
    bytes: &[u8],
    source: &str,
    source_url: Option<String>,
    market: Option<(String, String)>,
) -> Result<SkillCatalogItem, String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let skills_dir = crate::skills::service::skills_root_from_app_data(&app_dir);
    let stage = stage_skill_zip(bytes, &skills_dir)?;
    let skill = crate::skills::loader::SkillLoader::new(&skills_dir)
        .parse_skill_from_content(&stage.content)
        .map_err(|e| e.to_string())?;
    if skill.name.is_empty() || skill.name == "unnamed" {
        return Err("SKILL.md must declare a name".into());
    }
    let compat = check_skill_compatibility(&skill).await;
    if !compat.compatible {
        return Err(format!("Incompatible skill: {}", compat.issues.join("\n")));
    }
    let safe_name = crate::skills::service::sanitize_skill_id(&skill.name);
    validate_zip_path(&safe_name)?;
    let skill_dir = skill_install_destination(&skills_dir, &safe_name)?;
    let (market_id, market_version) = match market {
        Some((id, version)) => {
            let version = if version.is_empty() {
                skill.version.clone()
            } else {
                version
            };
            std::fs::write(
                stage.dir.join(".harmony-market.json"),
                serde_json::json!({ "id": id, "version": version }).to_string(),
            )
            .map_err(|e| e.to_string())?;
            (Some(id), Some(version))
        }
        None => (None, None),
    };
    {
        let db = state.db.lock().await;
        commit_skill_stage(
            &stage,
            &skill_dir,
            &db,
            &safe_name,
            &skill.name,
            &skill.description,
            source,
            source_url,
        )?;
    }
    notify_skills_changed(state);
    Ok(SkillCatalogItem {
        name: skill.name,
        description: skill.description,
        version: skill.version,
        source: "installed".into(),
        path: skill_dir.to_string_lossy().into_owned(),
        market_id,
        market_version,
        tools: skill.tools,
        dependencies: skill.dependencies,
        permissions: skill.permissions,
        platform: skill.platform,
    })
}

const MAX_SKILL_ZIP_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SKILL_ZIP_ENTRIES: usize = 4096;
const MAX_SKILL_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SKILL_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SKILL_MANIFEST_BYTES: u64 = 1024 * 1024;

fn skill_install_destination(
    root: &std::path::Path,
    id: &str,
) -> Result<std::path::PathBuf, String> {
    validate_zip_path(id)?;
    let installed = crate::skills::provenance::installed_dir(root);
    ensure_plain_directory(&installed)?;
    let current = installed.join(id);
    let legacy = root.join(id);
    // The loader migrates flat ZIP installs. Update their actual location rather
    // than creating a second flat copy that the older installed copy shadows.
    if std::fs::symlink_metadata(&current).is_ok()
        || !legacy.join("SKILL.md").is_file()
        || ["installed", "learned"].contains(&id)
        || crate::skills::provenance::is_builtin_dir_name(id)
    {
        Ok(current)
    } else {
        Ok(legacy)
    }
}

fn validate_zip_path(name: &str) -> Result<std::path::PathBuf, String> {
    use std::path::{Component, PathBuf};
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(format!("Absolute ZIP path: {name:?}"));
    }
    let normalized = normalized.strip_suffix('/').unwrap_or(&normalized);
    let mut path = PathBuf::new();
    for part in normalized.split('/') {
        let stem = part.split('.').next().unwrap_or_default().trim().to_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"]
            .contains(&stem.as_str())
            || (stem.starts_with("COM") || stem.starts_with("LPT"))
                && ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"]
                    .contains(&&stem[3..]);
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*".contains(c))
            || reserved
        {
            return Err(format!("Unsafe ZIP path component: {name:?}"));
        }
        path.push(part);
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("Unsafe ZIP path: {name:?}"));
    }
    Ok(path)
}

struct SkillZipEntry {
    name: std::path::PathBuf,
    directory: bool,
    size: u64,
}

fn inspect_skill_zip(bytes: &[u8]) -> Result<(Vec<SkillZipEntry>, usize), String> {
    use std::collections::HashMap;
    use zip::HasZipMetadata;
    if bytes.len() as u64 > MAX_SKILL_ZIP_BYTES {
        return Err("Skill ZIP exceeds download size limit".into());
    }
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > MAX_SKILL_ZIP_ENTRIES {
        return Err("Skill ZIP exceeds entry count limit".into());
    }
    let mut entries = Vec::new();
    let mut names = HashMap::new();
    let mut manifests = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = validate_zip_path(file.name())?;
        let directory = file.is_dir();
        let kind = file.unix_mode().unwrap_or(0) & 0o170000;
        let attrs = file.get_metadata().external_attributes;
        if file.is_symlink()
            || !matches!(kind, 0 | 0o100000 | 0o040000)
            || (kind == 0o040000 && !directory)
            || (kind == 0o100000 && directory)
            || attrs & 0x408 != 0
            || file.encrypted()
        {
            return Err(format!(
                "ZIP links, special files and encrypted entries are not allowed: {:?}",
                file.name()
            ));
        }
        // Legacy Unix/ASi fields can carry link targets; never materialize them.
        if let Some(mut extra) = file.extra_data() {
            while extra.len() >= 4 {
                let tag = u16::from_le_bytes([extra[0], extra[1]]);
                let len = u16::from_le_bytes([extra[2], extra[3]]) as usize;
                if matches!(tag, 0x000d | 0x756e) || len + 4 > extra.len() {
                    return Err("Unsupported ZIP link metadata".into());
                }
                extra = &extra[len + 4..];
            }
            if !extra.is_empty() {
                return Err("Truncated ZIP extra metadata".into());
            }
        }
        let key = name.to_string_lossy().replace('\\', "/").to_lowercase();
        if names.insert(key.clone(), directory).is_some() {
            return Err(format!("Duplicate/case-colliding ZIP path: {key}"));
        }
        let is_manifest = !directory
            && name.components().count() <= 2
            && name
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("SKILL.md"));
        let limit = if is_manifest {
            MAX_SKILL_MANIFEST_BYTES
        } else {
            MAX_SKILL_FILE_BYTES
        };
        total = total.checked_add(file.size()).ok_or("ZIP size overflow")?;
        if file.size() > limit || total > MAX_SKILL_TOTAL_BYTES || directory && file.size() != 0 {
            return Err("Skill ZIP exceeds expanded size limit".into());
        }
        if is_manifest {
            manifests.push(index);
        }
        entries.push(SkillZipEntry {
            name,
            directory,
            size: file.size(),
        });
    }
    for key in names.keys() {
        let mut parent = std::path::Path::new(key).parent();
        while let Some(p) = parent {
            if names.get(p.to_string_lossy().as_ref()) == Some(&false) {
                return Err("ZIP file/directory path collision".into());
            }
            parent = p.parent();
        }
    }
    if manifests.len() != 1 {
        return Err(
            "Skill ZIP must contain exactly one root or one-directory-deep SKILL.md".into(),
        );
    }
    Ok((entries, manifests[0]))
}

fn read_skill_zip(bytes: &[u8], destination: Option<&std::path::Path>) -> Result<String, String> {
    use std::io::Read;
    let (entries, manifest_index) = inspect_skill_zip(bytes)?;
    let prefix = entries[manifest_index]
        .name
        .parent()
        .ok_or("Missing manifest parent")?;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut manifest = Vec::new();
    let mut actual_total = 0u64;
    for (index, entry) in entries.iter().enumerate() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let relative = entry.name.strip_prefix(prefix).ok();
        if entry.directory {
            if let (Some(dest), Some(relative)) = (destination, relative) {
                std::fs::create_dir_all(dest.join(relative)).map_err(|e| e.to_string())?;
            }
            continue;
        }
        let mut reader = (&mut file).take(entry.size + 1);
        let written = if index == manifest_index {
            let n = std::io::copy(&mut reader, &mut manifest).map_err(|e| e.to_string())?;
            if let Some(dest) = destination {
                std::fs::write(dest.join("SKILL.md"), &manifest).map_err(|e| e.to_string())?;
            }
            n
        } else if let (Some(dest), Some(relative)) = (destination, relative) {
            let target = dest.join(relative);
            std::fs::create_dir_all(target.parent().ok_or("Missing ZIP parent")?)
                .map_err(|e| e.to_string())?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|e| e.to_string())?;
            let n = std::io::copy(&mut reader, &mut output).map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = if file.unix_mode().unwrap_or(0) & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                };
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(mode))
                    .map_err(|e| e.to_string())?;
            }
            n
        } else {
            std::io::copy(&mut reader, &mut std::io::sink()).map_err(|e| e.to_string())?
        };
        actual_total = actual_total
            .checked_add(written)
            .ok_or("ZIP size overflow")?;
        if written != entry.size || actual_total > MAX_SKILL_TOTAL_BYTES {
            return Err("ZIP expanded size does not match declared size".into());
        }
    }
    String::from_utf8(manifest)
        .map(|s| s.trim_start_matches('\u{FEFF}').to_string())
        .map_err(|e| e.to_string())
}

struct SkillStage {
    container: std::path::PathBuf,
    dir: std::path::PathBuf,
    content: String,
}

impl Drop for SkillStage {
    fn drop(&mut self) {
        // Never discard a recoverable old tree after a failed rollback/cleanup.
        if !self.container.join("backup").exists() {
            let _ = std::fs::remove_dir_all(&self.container);
        }
    }
}

fn ensure_plain_directory(path: &std::path::Path) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "Skill directory is a link or not a directory: {}",
                path.display()
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Skill directory is a Windows reparse point".into());
            }
        }
    } else {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn new_skill_stage(root: &std::path::Path) -> Result<SkillStage, String> {
    ensure_plain_directory(root)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let staging_root = root.join(".skill-installs");
    ensure_plain_directory(&staging_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staging_root, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let container = staging_root.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir(&container).map_err(|e| e.to_string())?;
    let stage = SkillStage {
        dir: container.join("payload"),
        container,
        content: String::new(),
    };
    std::fs::create_dir(&stage.dir).map_err(|e| e.to_string())?;
    Ok(stage)
}

fn stage_skill_zip(bytes: &[u8], root: &std::path::Path) -> Result<SkillStage, String> {
    inspect_skill_zip(bytes)?;
    let mut stage = new_skill_stage(root)?;
    stage.content = read_skill_zip(bytes, Some(&stage.dir))?;
    Ok(stage)
}

/// Each rename is same-filesystem and atomic. Keep the old tree until DB commit;
/// any ordinary error restores it. The backup is retained if rollback itself fails.
fn publish_skill_stage<F>(
    stage: &SkillStage,
    destination: &std::path::Path,
    register: F,
) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    use fs2::FileExt;
    let parent = destination.parent().ok_or("Missing skill parent")?;
    ensure_plain_directory(parent)?;
    let staging_root = stage.container.parent().ok_or("Missing staging parent")?;
    let root = staging_root.parent().ok_or("Missing skills root")?;
    let parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if parent != root && parent != root.join("installed") {
        return Err("Skill destination escaped the skills directory".into());
    }
    let id = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid skill destination")?;
    validate_zip_path(id)?;
    if parent == root
        && (["installed", "learned"].contains(&id)
            || crate::skills::provenance::is_builtin_dir_name(id))
    {
        return Err("Cannot replace a reserved skill directory".into());
    }
    let lock_path = staging_root.join("install.lock");
    if let Ok(meta) = std::fs::symlink_metadata(&lock_path) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err("Unsafe skill install lock".into());
        }
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    lock.lock_exclusive().map_err(|e| e.to_string())?;
    let exists = match std::fs::symlink_metadata(destination) {
        Ok(_) => {
            ensure_plain_directory(destination)?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    let backup = stage.container.join("backup");
    if exists {
        std::fs::rename(destination, &backup).map_err(|e| e.to_string())?;
    }
    let outcome = std::fs::rename(&stage.dir, destination)
        .map_err(|e| e.to_string())
        .and_then(|_| register());
    if let Err(error) = outcome {
        if destination.exists() {
            std::fs::rename(destination, &stage.dir).map_err(|e| {
                format!(
                    "{error}; cannot withdraw new skill: {e}; old skill retained at {}",
                    backup.display()
                )
            })?;
        }
        if exists {
            std::fs::rename(&backup, destination).map_err(|e| {
                format!(
                    "{error}; rollback failed: {e}; old skill retained at {}",
                    backup.display()
                )
            })?;
        }
        return Err(error);
    }
    if exists {
        if let Err(e) = std::fs::remove_dir_all(&backup) {
            warn!(
                "Skill installed; old backup retained at {}: {e}",
                backup.display()
            );
        }
    }
    Ok(())
}

fn commit_skill_stage(
    stage: &SkillStage,
    destination: &std::path::Path,
    db: &crate::store::Database,
    id: &str,
    name: &str,
    description: &str,
    source: &str,
    source_url: Option<String>,
) -> Result<(), String> {
    publish_skill_stage(stage, destination, || {
        let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let meta = crate::skills::provenance::SkillConfigMeta::installed(source, source_url, None);
        crate::skills::service::register_skill_db(db, id, name, description, &meta, Some(source))
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    })
}

fn commit_content_stage(
    stage: &SkillStage,
    destination: &std::path::Path,
    db: &crate::store::Database,
    root: &std::path::Path,
    source: &str,
    source_url: Option<String>,
) -> Result<(), String> {
    publish_skill_stage(stage, destination, || {
        let tx = db.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        // The legacy service writes/registers only after the complete staged tree
        // is published. Its errors now roll back both the DB and filesystem.
        crate::skills::service::install_to_installed(
            db,
            root,
            &stage.content,
            source,
            source_url,
            None,
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    })
}

async fn read_zip_response(response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|n| n > MAX_SKILL_ZIP_BYTES)
    {
        return Err("Skill ZIP exceeds download size limit".into());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len() as u64 + chunk.len() as u64 > MAX_SKILL_ZIP_BYTES {
            return Err("Skill ZIP exceeds download size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Remove an installed skill by name. Only skills whose source is "installed" or "workspace"
/// can be removed this way; built-in skills are protected.
#[tauri::command]
pub async fn uninstall_skill(state: State<'_, AppState>, skill_name: String) -> Result<(), String> {
    let app_dir = state
        .app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from(".xiaoheng"));
    let skills_dir = app_dir.join("skills");

    let safe_name: String = skill_name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase();

    // Remove from DB first — if the DB delete fails, abort before touching the filesystem
    {
        let db = state.db.lock().await;
        db.delete_skill(&safe_name)
            .map_err(|e| format!("Failed to remove skill from database: {}", e))?;
    }

    // Remove matching skill folders from disk by both directory id and parsed skill name.
    if skills_dir.exists() {
        let canonical_skills = skills_dir.canonicalize().map_err(|e| e.to_string())?;
        let mut loader = crate::skills::loader::SkillLoader::new(&skills_dir);
        let _ = loader.load_all();

        let mut candidate_dirs: std::collections::BTreeSet<std::path::PathBuf> =
            std::collections::BTreeSet::new();
        candidate_dirs.insert(skills_dir.join(&safe_name));
        for skill in loader.list_skills() {
            let parsed_safe_name = skill
                .name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .to_lowercase();
            if skill.name.eq_ignore_ascii_case(&skill_name) || parsed_safe_name == safe_name {
                candidate_dirs.insert(skill.source_path.clone());
            }
        }

        for skill_dir in candidate_dirs {
            if !skill_dir.exists() {
                continue;
            }
            let canonical_dir = skill_dir.canonicalize().map_err(|e| e.to_string())?;
            if canonical_dir == canonical_skills || !canonical_dir.starts_with(&canonical_skills) {
                return Err("Path traversal attempt blocked".into());
            }
            tokio::fs::remove_dir_all(&skill_dir).await.map_err(|e| {
                format!(
                    "Skill removed from database but failed to delete files: {}",
                    e
                )
            })?;
        }
    }

    info!("Uninstalled skill '{}'", skill_name);
    notify_skills_changed(&state);
    Ok(())
}

// ─── ClawHub Skill Registry ───────────────────────────────────────────────────

/// ClawHub public API base URL.
const CLAWHUB_API: &str = "https://clawhub.ai";
const HARMONY_MARKET_API: &str = crate::service_urls::MARKET_BASE;

/// A skill entry from the ClawHub registry.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ClawHubSkill {
    /// Unique skill slug on ClawHub (e.g. "my-skill").
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub downloads: u64,
    pub stars: u64,
    pub tags: Vec<String>,
    /// URL to fetch SKILL.md via `/api/v1/skills/<slug>/file?path=SKILL.md`
    pub skill_url: Option<String>,
    /// URL to download the zip bundle via `/api/v1/download?slug=<slug>`
    pub zip_url: Option<String>,
    /// OS/platform requirements from ClawHub metadata (e.g. ["windows"], ["linux"])
    pub platform: Vec<String>,
    /// Dependency requirements extracted from SKILL.md frontmatter (if pre-fetched)
    pub dependencies: Vec<String>,
    /// Whether this skill is compatible with the current system (None = not yet checked)
    pub compatible: Option<bool>,
    /// Compatibility issues (populated when compatible = false)
    pub compat_issues: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ClawHubSearchResult {
    pub items: Vec<ClawHubSkill>,
    pub total: usize,
    pub query: String,
}

fn harmony_market_base_url() -> String {
    std::env::var("HARMONY_MARKET_URL")
        .ok()
        .map(|s| crate::service_urls::migrate_public_service_url(s.trim()).trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| HARMONY_MARKET_API.to_string())
}

fn harmony_auth(mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if let (Ok(user), Ok(pass)) = (
        std::env::var("HARMONY_MARKET_BASIC_USER"),
        std::env::var("HARMONY_MARKET_BASIC_PASSWORD"),
    ) {
        if !user.trim().is_empty() {
            req = req.basic_auth(user, Some(pass));
        }
    }
    req
}

fn harmony_abs_url(base: &str, path_or_url: &str) -> String {
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        crate::service_urls::migrate_public_service_url(path_or_url)
    } else if path_or_url.starts_with('/') {
        format!("{}{}", base.trim_end_matches('/'), path_or_url)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), path_or_url)
    }
}

/// Search ClawHub for skills.
///
/// Uses vector search (`/api/v1/search?q=`) when a query is provided,
/// or the list endpoint (`/api/v1/skills?sort=stars`) when the query is empty.
#[tauri::command]
pub async fn clawhub_search(
    query: String,
    limit: Option<u32>,
) -> Result<ClawHubSearchResult, String> {
    let limit = limit.unwrap_or(20).min(50);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Harmony-Work/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let q = query.trim().to_string();

    // Choose endpoint: vector search when query is non-empty, list by stars otherwise
    let (url, use_search_endpoint) = if q.is_empty() {
        (
            format!("{}/api/v1/skills?sort=stars&limit={}", CLAWHUB_API, limit),
            false,
        )
    } else {
        (
            format!(
                "{}/api/v1/search?q={}&limit={}",
                CLAWHUB_API,
                urlencoding::encode(&q),
                limit
            ),
            true,
        )
    };
    info!("ClawHub search: {}", url);

    let resp = clawhub_get_with_retry(&client, &url, 3)
        .await
        .map_err(|e| {
            format!(
                "无法连接到 ClawHub（{}）：{}。请检查网络连接。",
                CLAWHUB_API, e
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let hint = if status.as_u16() == 429 {
            "（请求过于频繁，请稍后再试）".to_string()
        } else {
            String::new()
        };
        let body = resp.text().await.unwrap_or_default();
        let body_preview = if body.chars().count() > 300 {
            body.chars().take(300).collect::<String>()
        } else {
            body.clone()
        };
        return Err(format!(
            "ClawHub 返回错误 HTTP {}{}：{}",
            status, hint, body_preview
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("ClawHub 响应格式异常：{}", e))?;

    // Parse items from either endpoint format:
    // - /api/v1/search  → { results: [{ slug, displayName, summary, version, score }] }
    // - /api/v1/skills  → { items:   [{ slug, displayName, summary, tags, stats, latestVersion, metadata }] }
    let items: Vec<ClawHubSkill> = if use_search_endpoint {
        let results = body["results"].as_array().cloned().unwrap_or_default();
        results
            .iter()
            .filter_map(|r| {
                let slug = r["slug"].as_str().unwrap_or("").to_string();
                if slug.is_empty() {
                    return None;
                }
                let name = r["displayName"].as_str().unwrap_or(&slug).to_string();
                let description = r["summary"].as_str().unwrap_or("").to_string();
                let version = r["version"].as_str().unwrap_or("").to_string();
                let skill_url = Some(format!(
                    "{}/api/v1/skills/{}/file?path=SKILL.md",
                    CLAWHUB_API, slug
                ));
                let zip_url = Some(format!("{}/api/v1/download?slug={}", CLAWHUB_API, slug));
                Some(ClawHubSkill {
                    slug,
                    name,
                    description,
                    version,
                    author: String::new(),
                    downloads: 0,
                    stars: 0,
                    tags: vec![],
                    skill_url,
                    zip_url,
                    platform: vec![],
                    dependencies: vec![],
                    compatible: None,
                    compat_issues: vec![],
                })
            })
            .collect()
    } else {
        let raw_items = body["items"].as_array().cloned().unwrap_or_default();
        raw_items
            .iter()
            .filter_map(|item| {
                let slug = item["slug"].as_str().unwrap_or("").to_string();
                if slug.is_empty() {
                    return None;
                }
                let name = item["displayName"].as_str().unwrap_or(&slug).to_string();
                let description = item["summary"].as_str().unwrap_or("").to_string();
                let version = item["latestVersion"]["version"]
                    .as_str()
                    .unwrap_or("latest")
                    .to_string();

                // tags is an object { tag_name: versionId } in the list endpoint
                let tags: Vec<String> = item["tags"]
                    .as_object()
                    .map(|obj| obj.keys().cloned().collect())
                    .unwrap_or_default();

                let stats = &item["stats"];
                let downloads = stats["installsAllTime"]
                    .as_u64()
                    .or_else(|| stats["downloads"].as_u64())
                    .unwrap_or(0);
                let stars = stats["stars"].as_u64().unwrap_or(0);

                // OS platform from metadata (clawdis.os field)
                let platform: Vec<String> = item["metadata"]["os"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let skill_url = Some(format!(
                    "{}/api/v1/skills/{}/file?path=SKILL.md",
                    CLAWHUB_API, slug
                ));
                let zip_url = Some(format!("{}/api/v1/download?slug={}", CLAWHUB_API, slug));

                Some(ClawHubSkill {
                    slug,
                    name,
                    description,
                    version,
                    author: String::new(),
                    downloads,
                    stars,
                    tags,
                    skill_url,
                    zip_url,
                    platform,
                    dependencies: vec![],
                    compatible: None,
                    compat_issues: vec![],
                })
            })
            .collect()
    };

    let total = items.len();
    Ok(ClawHubSearchResult {
        items,
        total,
        query,
    })
}

#[tauri::command]
pub async fn harmony_market_search(
    query: String,
    limit: Option<u32>,
) -> Result<ClawHubSearchResult, String> {
    let limit = limit.unwrap_or(50).min(100) as usize;
    let base = harmony_market_base_url();
    let url = format!("{}/api/v1/skills", base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Harmony-WorkBuddy/0.7")
        .build()
        .map_err(|e| e.to_string())?;

    info!("Harmony skill market search: {}", url);
    let resp = harmony_auth(client.get(&url))
        .send()
        .await
        .map_err(|e| format!("Harmony market request failed: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Harmony market returned HTTP {}: {}", status, body));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Harmony market response is not valid JSON: {}", e))?;
    let q = query.trim().to_lowercase();
    let raw_items = body["skills"].as_array().cloned().unwrap_or_default();
    let mut items = Vec::new();
    for item in raw_items {
        let slug = item["id"].as_str().unwrap_or("").to_string();
        if slug.is_empty() {
            continue;
        }
        let name = item["name"].as_str().unwrap_or(&slug).to_string();
        let description = item["description"].as_str().unwrap_or("").to_string();
        let category = item["category"].as_str().unwrap_or("").to_string();
        let author = item["author"].as_str().unwrap_or("Harmony").to_string();
        let haystack = format!("{} {} {} {}", slug, name, description, category).to_lowercase();
        if !q.is_empty() && !haystack.contains(&q) {
            continue;
        }
        let version = item["version"].as_str().unwrap_or("latest").to_string();
        let zip_url = item["download_url"]
            .as_str()
            .map(|u| harmony_abs_url(&base, u));
        let tags = if category.is_empty() {
            vec!["harmony".to_string()]
        } else {
            vec!["harmony".to_string(), category]
        };
        items.push(ClawHubSkill {
            slug,
            name,
            description,
            version,
            author,
            downloads: 0,
            stars: 0,
            tags,
            skill_url: None,
            zip_url,
            platform: vec![],
            dependencies: vec![],
            compatible: None,
            compat_issues: vec![],
        });
        if items.len() >= limit {
            break;
        }
    }
    let total = items.len();
    Ok(ClawHubSearchResult {
        items,
        total,
        query,
    })
}

/// Pre-check whether a skill (from URL or local path) is compatible with the current system.
/// Returns compatibility info without actually installing the skill.
#[tauri::command]
pub async fn check_skill_compat(
    source: String,
) -> Result<crate::skills::loader::CompatibilityCheck, String> {
    let content = if source.starts_with("http://") || source.starts_with("https://") {
        let blocked = [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "192.168.",
            "10.",
            "172.",
        ];
        for pat in blocked {
            if source.contains(pat) {
                return Err(format!("Blocked URL: '{}'", source));
            }
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .get(&source)
            .header("User-Agent", "Harmony-Work/1.0")
            .send()
            .await
            .map_err(|e| format!("Download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} when fetching: {}", resp.status(), source));
        }
        resp.text().await.map_err(|e| e.to_string())?
    } else {
        tokio::fs::read_to_string(&source)
            .await
            .map_err(|e| format!("Failed to read '{}': {}", source, e))?
    };

    let loader = crate::skills::loader::SkillLoader::new(std::path::Path::new("."));
    let skill = loader
        .parse_skill_from_content(&content)
        .map_err(|e| format!("Failed to parse SKILL.md: {}", e))?;

    Ok(check_skill_compatibility(&skill).await)
}

#[cfg(test)]
mod security_zip_tests {
    use super::*;
    use std::fs;
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;

    const MANIFEST: &str = "---\nname: security-sample\ndescription: test\n---\n# Test";

    fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    struct TestRoot(std::path::PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("harmony-zip-security-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn security_zip_staging_keeps_manifest_below_scan_depth() {
        let root = TestRoot::new();
        let _stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        assert!(!fs::read_dir(&root.0)
            .unwrap()
            .flatten()
            .any(|e| e.path().join("SKILL.md").exists()));
    }

    #[test]
    fn security_zip_staging_is_not_discoverable_as_an_installed_skill() {
        let root = TestRoot::new();
        let _stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        let mut loader = crate::skills::loader::SkillLoader::new(&root.0);
        loader.load_all().unwrap();
        assert!(!loader
            .list_skills()
            .iter()
            .any(|s| s.name == "security-sample"));
    }

    #[test]
    fn security_zip_publish_preserves_helpers_and_replaces_whole_old_tree() {
        let root = TestRoot::new();
        let dest = root.0.join("security-sample");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("old-only"), "old").unwrap();
        let bytes = bundle(&[
            ("pkg/SKILL.md", MANIFEST.as_bytes()),
            ("pkg/scripts/helper.txt", b"helper"),
        ]);
        let stage = stage_skill_zip(&bytes, &root.0).unwrap();
        assert!(!dest.join("SKILL.md").exists());
        let db = crate::store::Database::open_in_memory().unwrap();
        assert!(db.get_skill("security-sample").unwrap().is_none());
        commit_skill_stage(
            &stage,
            &dest,
            &db,
            "security-sample",
            "security-sample",
            "new",
            "harmony-market",
            None,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("scripts/helper.txt")).unwrap(),
            "helper"
        );
        assert!(!dest.join("old-only").exists());
        assert!(db.get_skill("security-sample").unwrap().is_some());
    }

    #[test]
    fn security_zip_database_failure_restores_old_tree_and_old_record() {
        let root = TestRoot::new();
        let db = crate::store::Database::open_in_memory().unwrap();
        db.upsert_skill("security-sample", "Old name", "Old description", "old")
            .unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_skill_usage BEFORE INSERT ON skill_usage BEGIN SELECT RAISE(FAIL, 'injected DB failure'); END;").unwrap();
        let dest = root.0.join("security-sample");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "old bytes").unwrap();
        let stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        assert!(commit_skill_stage(
            &stage,
            &dest,
            &db,
            "security-sample",
            "new",
            "new",
            "manual",
            None
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(dest.join("SKILL.md")).unwrap(),
            "old bytes"
        );
        let record = db.get_skill("security-sample").unwrap().unwrap();
        assert_eq!(record.name, "Old name");
        assert_eq!(record.description, "Old description");
    }

    #[test]
    fn security_zip_new_install_db_failure_leaves_no_fake_record_or_directory() {
        let root = TestRoot::new();
        let db = crate::store::Database::open_in_memory().unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_skill_usage BEFORE INSERT ON skill_usage BEGIN SELECT RAISE(FAIL, 'injected DB failure'); END;").unwrap();
        let dest = root.0.join("security-sample");
        let stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        assert!(commit_skill_stage(
            &stage,
            &dest,
            &db,
            "security-sample",
            "new",
            "new",
            "clawhub",
            None
        )
        .is_err());
        assert!(!dest.exists());
        assert!(db.get_skill("security-sample").unwrap().is_none());
    }

    #[test]
    fn security_content_install_uses_the_same_rollback_boundary() {
        let root = TestRoot::new();
        let db = crate::store::Database::open_in_memory().unwrap();
        let dest = root.0.join("installed/security-sample");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "old").unwrap();
        let stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        db.conn.execute_batch("CREATE TRIGGER fail_skill_usage BEFORE INSERT ON skill_usage BEGIN SELECT RAISE(FAIL, 'injected DB failure'); END;").unwrap();
        assert!(commit_content_stage(&stage, &dest, &db, &root.0, "manual", None).is_err());
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "old");
        assert!(db.get_skill("security-sample").unwrap().is_none());
        db.conn
            .execute_batch("DROP TRIGGER fail_skill_usage;")
            .unwrap();
        commit_content_stage(&stage, &dest, &db, &root.0, "manual", None).unwrap();
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), MANIFEST);
    }

    #[test]
    fn security_zip_promotion_failure_does_not_register_or_destroy_old_skill() {
        let root = TestRoot::new();
        let dest = root.0.join("security-sample");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "old").unwrap();
        let stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        fs::remove_dir_all(&stage.dir).unwrap();
        let called = std::cell::Cell::new(false);
        assert!(publish_skill_stage(&stage, &dest, || {
            called.set(true);
            Ok(())
        })
        .is_err());
        assert!(!called.get());
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "old");
    }

    #[test]
    fn security_zip_bad_crc_cleans_staging_and_preserves_old_skill() {
        let root = TestRoot::new();
        let dest = root.0.join("security-sample");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "old").unwrap();
        let mut bytes = bundle(&[("SKILL.md", MANIFEST.as_bytes()), ("helper", b"new")]);
        let archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let offset = archive.central_directory_start() as usize;
        drop(archive);
        bytes[offset + 16] ^= 0xff;
        assert!(stage_skill_zip(&bytes, &root.0).is_err());
        assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "old");
        assert!(fs::read_dir(root.0.join(".skill-installs"))
            .unwrap()
            .next()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn security_zip_destination_symlink_cannot_touch_outside_sentinel() {
        let root = TestRoot::new();
        let outside = TestRoot::new();
        fs::write(outside.0.join("sentinel"), "untouched").unwrap();
        let dest = root.0.join("security-sample");
        std::os::unix::fs::symlink(&outside.0, &dest).unwrap();
        let stage =
            stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
        assert!(publish_skill_stage(&stage, &dest, || Ok(())).is_err());
        assert_eq!(
            fs::read_to_string(outside.0.join("sentinel")).unwrap(),
            "untouched"
        );
        assert!(!outside.0.join("SKILL.md").exists());
    }

    #[test]
    fn security_zip_rejects_windows_and_traversal_paths_even_outside_skill_prefix() {
        for name in [
            "C:/escape",
            "C:escape",
            "\\\\server\\share\\escape",
            "//server/share/escape",
            "\\\\?\\C:\\escape",
            "\\\\.\\NUL",
            "/absolute",
            "../escape",
            "pkg/../../escape",
            "pkg\\..\\escape",
            "pkg/file:stream",
            "pkg/NUL.txt",
            "pkg/NUL .txt",
            "pkg/COM1",
            "pkg/COM0.txt",
            "pkg/LPT¹.txt",
            "pkg/trailing. ",
        ] {
            let bytes = bundle(&[("pkg/SKILL.md", MANIFEST.as_bytes()), (name, b"sentinel")]);
            assert!(
                extract_skill_md_from_zip(&bytes).is_err(),
                "accepted {name:?}"
            );
        }
    }

    #[test]
    fn security_zip_rejects_links() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("SKILL.md", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(MANIFEST.as_bytes()).unwrap();
        writer
            .add_symlink("link", "../sentinel", SimpleFileOptions::default())
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(extract_skill_md_from_zip(&bytes).is_err());
    }

    #[test]
    fn security_zip_rejects_excessive_entry_count() {
        let names: Vec<_> = (0..4096).map(|i| format!("files/{i}")).collect();
        let mut entries = vec![("SKILL.md", MANIFEST.as_bytes())];
        entries.extend(names.iter().map(|name| (name.as_str(), b"".as_slice())));
        assert!(extract_skill_md_from_zip(&bundle(&entries)).is_err());
    }

    #[test]
    fn security_zip_rejects_oversized_expansion() {
        let data = vec![b'x'; 64 * 1024 * 1024 + 1];
        let bytes = bundle(&[("SKILL.md", MANIFEST.as_bytes()), ("huge", &data)]);
        assert!(extract_skill_md_from_zip(&bytes).is_err());
    }

    fn patch_entry(bytes: &mut [u8], index: usize, field: usize, value: u32) {
        let mut archive = zip::ZipArchive::new(Cursor::new(&*bytes)).unwrap();
        let offset = archive.by_index(index).unwrap().central_header_start() as usize;
        drop(archive);
        bytes[offset + field..offset + field + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn security_zip_rejects_total_size_manifest_size_and_forged_size() {
        let mut bytes = bundle(&[
            ("SKILL.md", MANIFEST.as_bytes()),
            ("a", b"a"),
            ("b", b"b"),
            ("c", b"c"),
            ("d", b"d"),
        ]);
        for index in 1..5 {
            patch_entry(&mut bytes, index, 24, MAX_SKILL_FILE_BYTES as u32);
        }
        assert!(extract_skill_md_from_zip(&bytes).is_err());
        let mut bytes = bundle(&[("SKILL.md", MANIFEST.as_bytes())]);
        patch_entry(&mut bytes, 0, 24, MAX_SKILL_MANIFEST_BYTES as u32 + 1);
        assert!(extract_skill_md_from_zip(&bytes).is_err());
        let mut bytes = bundle(&[("SKILL.md", MANIFEST.as_bytes()), ("a", b"actual bytes")]);
        patch_entry(&mut bytes, 1, 24, 0);
        assert!(extract_skill_md_from_zip(&bytes).is_err());
    }

    #[test]
    fn security_zip_rejects_windows_reparse_and_special_file_metadata() {
        for attrs in [0x400, 0x08, 0o020644 << 16, 0o010644 << 16] {
            let mut bytes = bundle(&[("SKILL.md", MANIFEST.as_bytes()), ("special", b"target")]);
            patch_entry(&mut bytes, 1, 38, attrs);
            assert!(
                extract_skill_md_from_zip(&bytes).is_err(),
                "accepted attributes {attrs:x}"
            );
        }
    }

    #[test]
    fn security_zip_cannot_replace_reserved_installation_roots() {
        let root = TestRoot::new();
        for id in ["installed", "learned", "office-automation"] {
            let stage =
                stage_skill_zip(&bundle(&[("SKILL.md", MANIFEST.as_bytes())]), &root.0).unwrap();
            let dest = root.0.join(id);
            fs::create_dir(&dest).unwrap();
            fs::write(dest.join("sentinel"), "untouched").unwrap();
            assert!(publish_skill_stage(&stage, &dest, || Ok(())).is_err());
            assert_eq!(
                fs::read_to_string(dest.join("sentinel")).unwrap(),
                "untouched"
            );
        }
    }

    #[test]
    fn security_zip_updates_migrated_or_legacy_location_without_shadow_copy() {
        let root = TestRoot::new();
        assert_eq!(
            skill_install_destination(&root.0, "sample").unwrap(),
            root.0.join("installed/sample")
        );
        let legacy = root.0.join("sample");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("SKILL.md"), MANIFEST).unwrap();
        assert_eq!(
            skill_install_destination(&root.0, "sample").unwrap(),
            legacy
        );
        let current = root.0.join("installed/sample");
        fs::create_dir(&current).unwrap();
        fs::write(current.join("SKILL.md"), MANIFEST).unwrap();
        assert_eq!(
            skill_install_destination(&root.0, "sample").unwrap(),
            current
        );
        assert_eq!(
            skill_install_destination(&root.0, "installed").unwrap(),
            root.0.join("installed/installed")
        );
    }

    #[test]
    fn security_zip_rejects_ambiguous_manifests_and_case_collisions() {
        for entries in [
            vec![
                ("SKILL.md", MANIFEST.as_bytes()),
                ("other/skill.md", MANIFEST.as_bytes()),
            ],
            vec![
                ("SKILL.md", MANIFEST.as_bytes()),
                ("File", b"a"),
                ("file", b"b"),
            ],
        ] {
            assert!(extract_skill_md_from_zip(&bundle(&entries)).is_err());
        }
    }

    #[test]
    fn security_zip_accepts_windows_separators_and_literal_metacharacters() {
        let bytes = bundle(&[
            ("pkg\\SKILL.md", MANIFEST.as_bytes()),
            ("pkg\\资料 & (a)..txt", b"ok"),
        ]);
        assert_eq!(extract_skill_md_from_zip(&bytes).unwrap(), MANIFEST);
    }
}

#[cfg(test)]
mod runtime_verification_tests {
    use super::{read_market_metadata, verify_skill_runtime_from, SkillRuntimeState};
    use crate::store::db::Skill;
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    fn record(enabled: bool) -> Skill {
        Skill {
            id: "sample-skill".into(),
            name: "Sample Skill".into(),
            description: String::new(),
            enabled,
            icon: "📦".into(),
            config: r#"{"lifecycle":"installed"}"#.into(),
        }
    }

    fn skills_root(test_name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "harmony-work-runtime-verification-{}-{}",
            test_name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("installed/sample-skill")).unwrap();
        root
    }

    fn write_skill(root: &PathBuf, dependencies: &[&str]) {
        let dependencies = dependencies
            .iter()
            .map(|dependency| format!("  - {dependency}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            root.join("installed/sample-skill/SKILL.md"),
            format!(
                "---\nname: Sample Skill\ndescription: test\ndependencies:\n{dependencies}\n---\n\n# Test"
            ),
        )
        .unwrap();
    }

    #[test]
    fn reads_market_identity_and_version_from_installed_skill_directory() {
        let root = skills_root("market-metadata");
        let skill_dir = root.join("installed/sample-skill");
        fs::write(
            skill_dir.join(".harmony-market.json"),
            r#"{"id":"ppt-master","version":"6.6.0+main.481e057"}"#,
        )
        .unwrap();
        assert_eq!(
            read_market_metadata(&skill_dir),
            (Some("ppt-master".into()), Some("6.6.0+main.481e057".into()))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verifies_an_enabled_skill_with_available_dependencies() {
        let root = skills_root("ready");
        write_skill(&root, &["node", "mcp:company"]);

        let result = verify_skill_runtime_from(
            "sample-skill",
            Some(&record(true)),
            &root,
            &HashSet::from(["company".to_string()]),
            |command| command == "node",
        );

        assert!(result.installed);
        assert!(result.dependencies_ready);
        assert_eq!(result.runtime_state, SkillRuntimeState::Ready);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_a_missing_skill_manifest_as_repair_required() {
        let root = skills_root("missing-manifest");

        let result = verify_skill_runtime_from(
            "sample-skill",
            Some(&record(true)),
            &root,
            &HashSet::new(),
            |_| true,
        );

        assert!(!result.installed);
        assert_eq!(result.runtime_state, SkillRuntimeState::RepairRequired);
        assert!(result.repair_hint.unwrap().contains("SKILL.md"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_missing_command_dependencies_without_executing_the_skill() {
        let root = skills_root("missing-command");
        write_skill(&root, &["node"]);

        let result = verify_skill_runtime_from(
            "sample-skill",
            Some(&record(true)),
            &root,
            &HashSet::new(),
            |_| false,
        );

        assert!(!result.dependencies_ready);
        assert_eq!(result.runtime_state, SkillRuntimeState::RepairRequired);
        assert_eq!(result.missing_dependencies, vec!["node"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn does_not_mark_a_disabled_record_as_ready() {
        let root = skills_root("disabled");
        write_skill(&root, &[]);

        let result = verify_skill_runtime_from(
            "sample-skill",
            Some(&record(false)),
            &root,
            &HashSet::new(),
            |_| true,
        );

        assert!(!result.enabled);
        assert_eq!(result.runtime_state, SkillRuntimeState::RepairRequired);
        let _ = fs::remove_dir_all(root);
    }
}

/// Install a skill from ClawHub by slug.
/// Fetches SKILL.md via `/api/v1/skills/<slug>/file?path=SKILL.md`,
/// falls back to the zip download if the file endpoint fails.
#[tauri::command]
pub async fn clawhub_install(
    state: State<'_, AppState>,
    slug: String,
    version: Option<String>,
) -> Result<SkillCatalogItem, String> {
    // Validate slug — only allow alphanumeric, hyphens, underscores, dots
    if !slug
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!("无效的技能 slug：'{}'", slug));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Harmony-Work/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let normalized_version = version
        .as_deref()
        .map(str::trim)
        .filter(|ver| !ver.is_empty() && *ver != "latest" && *ver != "null");

    // Build the file URL, optionally pinning a concrete version
    let file_url = if let Some(ver) = normalized_version {
        format!(
            "{}/api/v1/skills/{}/file?path=SKILL.md&version={}",
            CLAWHUB_API, slug, ver
        )
    } else {
        format!("{}/api/v1/skills/{}/file?path=SKILL.md", CLAWHUB_API, slug)
    };
    info!(
        "ClawHub install: fetching SKILL.md for '{}' from {}",
        slug, file_url
    );

    let resp = clawhub_get_with_retry(&client, &file_url, 3)
        .await
        .map_err(|e| format!("下载失败：{}", e))?;

    let content = if resp.status().is_success() {
        resp.text()
            .await
            .map_err(|e| format!("读取 SKILL.md 失败：{}", e))?
    } else {
        let file_status = resp.status();
        // Fallback: download the zip bundle and extract SKILL.md
        let zip_url = if let Some(ver) = normalized_version {
            format!(
                "{}/api/v1/download?slug={}&version={}",
                CLAWHUB_API, slug, ver
            )
        } else {
            format!("{}/api/v1/download?slug={}", CLAWHUB_API, slug)
        };
        info!(
            "ClawHub: file endpoint returned {}, trying zip: {}",
            file_status, zip_url
        );
        let zip_resp = clawhub_get_with_retry(&client, &zip_url, 3)
            .await
            .map_err(|e| format!("Zip 下载失败：{}", e))?;
        if !zip_resp.status().is_success() {
            let hint = if zip_resp.status().as_u16() == 429 {
                "请求过于频繁，请稍后再试".to_string()
            } else {
                format!("HTTP {}", zip_resp.status())
            };
            return Err(format!("ClawHub：技能 '{}' 安装失败（{}）", slug, hint));
        }
        let zip_bytes = read_zip_response(zip_resp).await?;
        return install_skill_archive(
            &state,
            &zip_bytes,
            "clawhub",
            Some(format!("{}/skills/{}", CLAWHUB_API, slug)),
            None,
        )
        .await;
    };

    let source_url = Some(format!("{}/skills/{}", CLAWHUB_API, slug));
    install_skill_from_content_sourced(&state, content, "clawhub", source_url).await
}

#[tauri::command]
pub async fn harmony_market_install(
    state: State<'_, AppState>,
    slug: String,
) -> Result<SkillCatalogItem, String> {
    if !slug
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!("Invalid Harmony skill id: '{}'", slug));
    }

    let base = harmony_market_base_url();
    let market_version = harmony_market_search(String::new(), Some(100))
        .await
        .ok()
        .and_then(|result| result.items.into_iter().find(|item| item.slug == slug))
        .map(|item| item.version);
    let existing = scan_skill_catalog(state.clone()).await?;
    if let Some(item) = existing
        .into_iter()
        .find(|item| item.market_id.as_deref() == Some(slug.as_str()))
    {
        if market_version.is_some() && market_version.as_deref() == item.market_version.as_deref() {
            emit_harmony_install_progress(&state, &slug, "complete", 100);
            return Ok(item);
        }
    }
    let url = format!("{}/api/v1/skills/{}/download", base, slug);
    emit_harmony_install_progress(&state, &slug, "downloading", 0);
    info!("Harmony market install: {}", url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("Harmony-WorkBuddy/0.7")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = harmony_auth(client.get(&url))
        .send()
        .await
        .map_err(|e| format!("Harmony skill download failed: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Harmony skill download returned HTTP {}: {}",
            status, body
        ));
    }
    let total = resp.content_length();
    if total.is_some_and(|n| n > MAX_SKILL_ZIP_BYTES) {
        return Err("Skill ZIP exceeds download size limit".into());
    }
    let mut bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Failed to read Harmony skill package: {}", e))?;
        if bytes.len() as u64 + chunk.len() as u64 > MAX_SKILL_ZIP_BYTES {
            return Err("Skill ZIP exceeds download size limit".into());
        }
        bytes.extend_from_slice(&chunk);
        let percent = total
            .filter(|total| *total > 0)
            .map(|total| ((bytes.len() as u64 * 85 / total).min(85)) as u8)
            .unwrap_or(0);
        emit_harmony_install_progress(&state, &slug, "downloading", percent);
    }
    emit_harmony_install_progress(&state, &slug, "installing", 90);
    let item = install_skill_archive(
        &state,
        &bytes,
        "harmony-market",
        Some(url),
        Some((slug.clone(), market_version.unwrap_or_default())),
    )
    .await?;
    emit_harmony_install_progress(&state, &slug, "complete", 100);
    Ok(item)
}

/// Extract SKILL.md text from a zip archive bytes.
#[cfg(test)]
fn extract_skill_md_from_zip(zip_bytes: &[u8]) -> Result<String, String> {
    read_skill_zip(zip_bytes, None)
}

/// Call Codex to generate bilingual (Chinese + English) trigger keywords for a skill,
/// then merge them into the SKILL.md frontmatter and write the file back.
///
/// This runs as a background task after installation — failures are non-fatal.
async fn enrich_triggers_with_codex(
    state: &AppState,
    skill: &crate::skills::loader::SkillDefinition,
    skill_file: &std::path::Path,
) -> anyhow::Result<()> {
    use tokio::time::{timeout, Duration};

    let existing_triggers = if skill.triggers.is_empty() {
        String::new()
    } else {
        format!("\nExisting triggers: {}", skill.triggers.join(", "))
    };

    let prompt = format!(
        "You are a multilingual keyword expert. Given this skill:\n\
         Name: {name}\n\
         Description: {desc}{existing}\n\n\
         Generate 10-20 trigger keywords in both Chinese and English that a user might say \
         when they need this skill. Include synonyms, abbreviations, and common phrases. \
         Return ONLY a JSON array of strings, no explanation, no markdown fences.\n\
         Example: [\"pptx\",\"PPT\",\"幻灯片\",\"演示文稿\",\"presentation\",\"slideshow\"]",
        name = skill.name,
        desc = skill.description,
        existing = existing_triggers,
    );

    let response = timeout(
        Duration::from_secs(20),
        crate::codex_runtime::background::run_codex_transient(
            state,
            &prompt,
            Some("Return only the requested JSON array. Do not call tools."),
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Codex trigger enrichment timed out"))?
    .map_err(|error| anyhow::anyhow!("Codex error: {}", error))?;

    let text = response.final_message;

    // Extract JSON array from response (may be wrapped in prose)
    let json_start = text
        .find('[')
        .ok_or_else(|| anyhow::anyhow!("No JSON array in response"))?;
    let json_end = text
        .rfind(']')
        .ok_or_else(|| anyhow::anyhow!("No closing ] in response"))?;
    let json_str = &text[json_start..=json_end];

    let new_triggers: Vec<String> = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse trigger JSON: {}", e))?;

    if new_triggers.is_empty() {
        return Ok(());
    }

    // Merge with existing triggers, deduplicating case-insensitively
    let mut merged: Vec<String> = skill.triggers.clone();
    let existing_lower: std::collections::HashSet<String> =
        merged.iter().map(|t| t.to_lowercase()).collect();
    for t in new_triggers {
        let t = t.trim().to_string();
        if !t.is_empty() && !existing_lower.contains(&t.to_lowercase()) {
            merged.push(t);
        }
    }

    // Read current SKILL.md content
    let current = tokio::fs::read_to_string(skill_file)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read SKILL.md: {}", e))?;

    // Build the new triggers YAML block
    let triggers_yaml = merged
        .iter()
        .map(|t| format!("  - \"{}\"", t.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join("\n");
    let triggers_block = format!("triggers:\n{}", triggers_yaml);

    // Replace or insert triggers block in frontmatter
    let updated = if current.contains("triggers:") {
        // Replace existing triggers block (handles multi-line list)
        let re = regex::Regex::new(r"(?m)^triggers:(\n  - [^\n]*)*")
            .map_err(|e| anyhow::anyhow!("Regex error: {}", e))?;
        re.replace(&current, triggers_block.as_str()).into_owned()
    } else {
        // Insert before closing --- of frontmatter
        if let Some(pos) = current.find("\n---\n") {
            let (front, rest) = current.split_at(pos);
            format!("{}\n{}{}", front, triggers_block, rest)
        } else {
            // No frontmatter end found, append to end of frontmatter section
            current.replacen("---", &format!("---\n{}", triggers_block), 2)
        }
    };

    tokio::fs::write(skill_file, updated)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to write enriched SKILL.md: {}", e))?;

    info!(
        "Enriched triggers for skill '{}': {} keywords",
        skill.name,
        merged.len()
    );
    Ok(())
}
