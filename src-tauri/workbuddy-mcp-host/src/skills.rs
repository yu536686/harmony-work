use crate::HostContext;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const CLAWHUB_API: &str = "https://clawhub.ai";
const MAX_SKILL_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "skill_market_search" | "skill_install" | "skill_set_enabled" | "skill_uninstall"
    )
}

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"skill_market_search",
            "description":"Search the ClawHub skill marketplace. Use local skill_search first when looking for an already installed capability.",
            "inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20,"default":10}},"additionalProperties":false},
            "annotations":{"title":"搜索技能市场","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
        json!({
            "name":"skill_install",
            "description":"Install a WorkBuddy skill from a ClawHub slug or a public direct SKILL.md/.zip URL. The installed skill is immediately registered for the UI and future Codex turns.",
            "inputSchema":{"type":"object","properties":{"source":{"type":"string","minLength":1},"enabled":{"type":"boolean","default":true}},"required":["source"],"additionalProperties":false},
            "annotations":{"title":"安装技能","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}
        }),
        json!({
            "name":"skill_set_enabled",
            "description":"Enable or disable an installed WorkBuddy skill by skill ID.",
            "inputSchema":{"type":"object","properties":{"skill_id":{"type":"string","minLength":1},"enabled":{"type":"boolean"}},"required":["skill_id","enabled"],"additionalProperties":false},
            "annotations":{"title":"启用或停用技能","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true}
        }),
        json!({
            "name":"skill_uninstall",
            "description":"Uninstall one user-installed WorkBuddy skill. Built-in skills and draft/learned skills are not removed by this tool.",
            "inputSchema":{"type":"object","properties":{"skill_id":{"type":"string","minLength":1}},"required":["skill_id"],"additionalProperties":false},
            "annotations":{"title":"卸载技能","readOnlyHint":false,"destructiveHint":true,"idempotentHint":true}
        }),
    ]
}

pub(crate) async fn call(context: &HostContext, name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "skill_market_search" => market_search(arguments).await,
        "skill_install" => install(context, arguments).await,
        "skill_set_enabled" => set_enabled(context, arguments),
        "skill_uninstall" => uninstall(context, arguments).await,
        _ => Err(anyhow!("unknown skill tool: {name}")),
    }
}

async fn market_search(arguments: &Value) -> Result<Value> {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 20);
    let url = if query.is_empty() {
        format!("{CLAWHUB_API}/api/v1/skills?sort=stars&limit={limit}")
    } else {
        format!(
            "{CLAWHUB_API}/api/v1/search?q={}&limit={limit}",
            urlencoding::encode(query)
        )
    };
    let payload = client()?
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    mcp_success(payload)
}

async fn install(context: &HostContext, arguments: &Value) -> Result<Value> {
    let source = required_string(arguments, "source")?;
    let source = crate::service_urls::migrate_public_service_url(&source);
    let enabled = arguments
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let downloaded = download_source(&source).await?;
    let staging = context
        .app_data_dir
        .join(".skill-install")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&staging)?;
    let skill_root = if downloaded.is_zip {
        extract_zip(&downloaded.bytes, &staging)?
    } else {
        std::fs::write(staging.join("SKILL.md"), &downloaded.bytes)?;
        staging.clone()
    };
    let skill_md = skill_root.join("SKILL.md");
    let content =
        std::fs::read_to_string(&skill_md).context("installed package has no readable SKILL.md")?;
    let metadata = parse_skill_metadata(&content)?;
    let skill_id = sanitize_skill_id(&metadata.name);
    if skill_id.is_empty() {
        return Err(anyhow!("SKILL.md must declare a valid name"));
    }
    let installed_root = context.app_data_dir.join("skills").join("installed");
    std::fs::create_dir_all(&installed_root)?;
    let target = installed_root.join(&skill_id);
    let prepared = installed_root.join(format!(".{skill_id}.staging-{}", uuid::Uuid::new_v4()));
    copy_directory(&skill_root, &prepared)?;
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&prepared, &target)?;
    let _ = std::fs::remove_dir_all(&staging);
    register_skill(context, &skill_id, &metadata, enabled, &source)?;
    mcp_success(json!({
        "ok":true,"skillId":skill_id,"name":metadata.name,"description":metadata.description,
        "enabled":enabled,"path":target,"source":source
    }))
}

fn set_enabled(context: &HostContext, arguments: &Value) -> Result<Value> {
    let skill_id = sanitize_skill_id(&required_string(arguments, "skill_id")?);
    let enabled = arguments
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| anyhow!("enabled is required"))?;
    let changed = update_registry_enabled(context, &skill_id, enabled)?;
    if !changed {
        return Err(anyhow!("skill not found: {skill_id}"));
    }
    mcp_success(json!({"ok":true,"skillId":skill_id,"enabled":enabled}))
}

async fn uninstall(context: &HostContext, arguments: &Value) -> Result<Value> {
    let skill_id = sanitize_skill_id(&required_string(arguments, "skill_id")?);
    let target = context
        .app_data_dir
        .join("skills")
        .join("installed")
        .join(&skill_id);
    let installed_root = context.app_data_dir.join("skills").join("installed");
    if !target.starts_with(&installed_root) {
        return Err(anyhow!("invalid skill path"));
    }
    if target.exists() {
        tokio::fs::remove_dir_all(&target).await?;
    }
    delete_registry_skill(context, &skill_id)?;
    mcp_success(json!({"ok":true,"skillId":skill_id,"removedPath":target}))
}

struct DownloadedSource {
    bytes: Vec<u8>,
    is_zip: bool,
}

async fn download_source(source: &str) -> Result<DownloadedSource> {
    let client = client()?;
    if source.starts_with("http://") || source.starts_with("https://") {
        validate_public_url(source)?;
        let response = client.get(source).send().await?.error_for_status()?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let bytes = response.bytes().await?.to_vec();
        check_download_size(&bytes)?;
        return Ok(DownloadedSource {
            is_zip: source.to_ascii_lowercase().ends_with(".zip") || content_type.contains("zip"),
            bytes,
        });
    }
    if !source
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(anyhow!("invalid ClawHub slug"));
    }
    let zip_url = format!("{CLAWHUB_API}/api/v1/download?slug={source}");
    let zip_response = client.get(&zip_url).send().await?;
    if zip_response.status().is_success() {
        let bytes = zip_response.bytes().await?.to_vec();
        check_download_size(&bytes)?;
        return Ok(DownloadedSource {
            bytes,
            is_zip: true,
        });
    }
    let file_url = format!("{CLAWHUB_API}/api/v1/skills/{source}/file?path=SKILL.md");
    let bytes = client
        .get(file_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();
    check_download_size(&bytes)?;
    Ok(DownloadedSource {
        bytes,
        is_zip: false,
    })
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("Harmony-Work-MCP/1.0")
        .build()?)
}

fn validate_public_url(value: &str) -> Result<()> {
    let url = reqwest::Url::parse(value)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL has no host"))?
        .to_ascii_lowercase();
    let private_ip = host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| match address {
            std::net::IpAddr::V4(address) => {
                address.is_private()
                    || address.is_loopback()
                    || address.is_link_local()
                    || address.is_unspecified()
            }
            std::net::IpAddr::V6(address) => address.is_loopback() || address.is_unspecified(),
        });
    if host == "localhost" || host.ends_with(".local") || private_ip {
        return Err(anyhow!("private network skill URLs are not allowed"));
    }
    Ok(())
}

fn check_download_size(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_SKILL_DOWNLOAD_BYTES {
        return Err(anyhow!("skill package must be between 1 byte and 50MB"));
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], staging: &Path) -> Result<PathBuf> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid skill zip")?;
    if archive.len() > 1_000 {
        return Err(anyhow!("skill zip contains too many files"));
    }
    let mut skill_roots = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("skill zip contains an unsafe path"))?
            .to_path_buf();
        if enclosed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        }) {
            return Err(anyhow!("skill zip contains an unsafe path"));
        }
        let target = staging.join(&enclosed);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&target)?;
        std::io::copy(
            &mut entry.take(MAX_SKILL_DOWNLOAD_BYTES as u64 + 1),
            &mut output,
        )?;
        if target.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
            if let Some(parent) = target.parent() {
                skill_roots.push(parent.to_path_buf());
            }
        }
    }
    skill_roots.sort_by_key(|path| path.components().count());
    skill_roots
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("skill zip does not contain SKILL.md"))
}

fn parse_skill_metadata(content: &str) -> Result<SkillFrontmatter> {
    let yaml = content
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("---"))
        .map(|(yaml, _)| yaml)
        .ok_or_else(|| anyhow!("SKILL.md must start with YAML frontmatter"))?;
    let metadata: SkillFrontmatter = serde_yaml::from_str(yaml)?;
    if metadata.name.trim().is_empty() {
        return Err(anyhow!("SKILL.md frontmatter requires name"));
    }
    Ok(metadata)
}

fn sanitize_skill_id(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

fn copy_directory(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target_path = target.join(entry.file_name());
        if entry.path().is_dir() {
            copy_directory(&entry.path(), &target_path)?;
        } else {
            std::fs::copy(entry.path(), target_path)?;
        }
    }
    Ok(())
}

fn register_skill(
    context: &HostContext,
    id: &str,
    metadata: &SkillFrontmatter,
    enabled: bool,
    source: &str,
) -> Result<()> {
    let database = Connection::open(&context.database_path)?;
    database.execute_batch("CREATE TABLE IF NOT EXISTS skill_registry (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, source TEXT NOT NULL, updated_at TEXT NOT NULL);")?;
    database.execute("INSERT INTO skill_registry (id,name,description,enabled,source,updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,enabled=excluded.enabled,source=excluded.source,updated_at=excluded.updated_at", params![id, metadata.name, metadata.description, enabled as i64, source, chrono::Utc::now().to_rfc3339()])?;
    if let Some(legacy) = open_legacy(context)? {
        let config = json!({"lifecycle":"installed","locked":true,"pinned":false,"source":"workbuddy-mcp","source_url":source}).to_string();
        legacy.execute("INSERT INTO skills (id,name,description,enabled,icon,config) VALUES (?1,?2,?3,?4,'📦',?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,enabled=excluded.enabled,config=excluded.config", params![id, metadata.name, metadata.description, enabled as i64, config])?;
        legacy.execute("INSERT INTO skill_usage (skill_id,created_by) VALUES (?1,'workbuddy-mcp') ON CONFLICT(skill_id) DO NOTHING", params![id])?;
    }
    Ok(())
}

fn update_registry_enabled(context: &HostContext, id: &str, enabled: bool) -> Result<bool> {
    let mut changed = false;
    let database = Connection::open(&context.database_path)?;
    if table_exists(&database, "skill_registry")? {
        changed |= database.execute(
            "UPDATE skill_registry SET enabled=?1, updated_at=?2 WHERE id=?3",
            params![enabled as i64, chrono::Utc::now().to_rfc3339(), id],
        )? > 0;
    }
    if let Some(legacy) = open_legacy(context)? {
        changed |= legacy.execute(
            "UPDATE skills SET enabled=?1 WHERE id=?2",
            params![enabled as i64, id],
        )? > 0;
    }
    Ok(changed)
}

fn delete_registry_skill(context: &HostContext, id: &str) -> Result<()> {
    let database = Connection::open(&context.database_path)?;
    if table_exists(&database, "skill_registry")? {
        database.execute("DELETE FROM skill_registry WHERE id=?1", params![id])?;
    }
    if let Some(legacy) = open_legacy(context)? {
        legacy.execute("DELETE FROM skills WHERE id=?1", params![id])?;
        legacy.execute("DELETE FROM skill_usage WHERE skill_id=?1", params![id])?;
    }
    Ok(())
}

fn open_legacy(context: &HostContext) -> Result<Option<Connection>> {
    match context
        .legacy_database_path
        .as_ref()
        .filter(|path| path.is_file())
    {
        Some(path) => Ok(Some(Connection::open(path)?)),
        None => Ok(None),
    }
}

fn table_exists(database: &Connection, name: &str) -> Result<bool> {
    Ok(database.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        params![name],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn required_string(arguments: &Value, key: &str) -> Result<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{key} is required"))
}

fn mcp_success(payload: Value) -> Result<Value> {
    Ok(
        json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&payload)?}],"structuredContent":payload,"isError":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_ids_and_rejects_private_urls() {
        assert_eq!(sanitize_skill_id("My Skill"), "my_skill");
        assert!(validate_public_url("http://127.0.0.1/SKILL.md").is_err());
        assert!(validate_public_url("https://example.com/SKILL.md").is_ok());
    }
}
