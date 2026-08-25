use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod harvis;
mod im;
mod office;
mod process;
mod skills;
mod staffdeck;

#[derive(Debug, Clone)]
pub struct HostContext {
    pub app_data_dir: PathBuf,
    pub database_path: PathBuf,
    pub legacy_database_path: Option<PathBuf>,
    pub skills_roots: Vec<PathBuf>,
    pub marketing_base_url: String,
}

impl HostContext {
    pub fn new(
        app_data_dir: PathBuf,
        database_path: PathBuf,
        legacy_database_path: Option<PathBuf>,
        skills_roots: Vec<PathBuf>,
        marketing_base_url: String,
    ) -> Self {
        Self {
            app_data_dir,
            database_path,
            legacy_database_path,
            skills_roots,
            marketing_base_url,
        }
    }

    pub(crate) fn marketing_cookie_path(&self) -> PathBuf {
        self.app_data_dir.join("marketing.session.cookie")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryRecord {
    id: String,
    owner_id: String,
    content: String,
    category: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillRecord {
    name: String,
    description: String,
    path: String,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

pub async fn handle_request(context: &HostContext, request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(initialize_result(request)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => call_tool(context, request.get("params").unwrap_or(&Value::Null)).await,
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("Method not found: {method}")}
            }));
        }
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
        Err(error) => json!({"jsonrpc":"2.0", "id":id, "result":tool_error(error.to_string())}),
    })
}

fn initialize_result(request: &Value) -> Value {
    let requested = request
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": requested,
        "capabilities": {
            "tools": {"listChanged": false},
            "resources": {"subscribe": false, "listChanged": false}
        },
        "serverInfo": {
            "name": "workbuddy-mcp-host",
            "title": "Xiaoheng WorkBuddy MCP Host",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tool_definitions() -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "memory_search",
            "description": "Search Xiaoheng long-term memory by keyword. Returns matching memory IDs, categories, content, and timestamps. Use before appending to avoid duplicates.",
            "inputSchema": {
                "type":"object",
                "properties": {
                    "query":{"type":"string","minLength":1,"description":"Keyword or short phrase to search"},
                    "owner_id":{"type":"string","default":"xiaoheng"},
                    "limit":{"type":"integer","minimum":1,"maximum":50,"default":10}
                },
                "required":["query"],
                "additionalProperties":false
            }
        }),
        json!({
            "name": "memory_append",
            "description": "Append one durable user preference, project fact, or corrected lesson to Xiaoheng long-term memory. Do not store secrets or transient task progress.",
            "inputSchema": {
                "type":"object",
                "properties": {
                    "content":{"type":"string","minLength":1,"maxLength":8000},
                    "category":{"type":"string","default":"general"},
                    "owner_id":{"type":"string","default":"xiaoheng"}
                },
                "required":["content"],
                "additionalProperties":false
            }
        }),
        json!({
            "name": "skill_list",
            "description": "List installed WorkBuddy skills from all configured skill roots. Returns each skill name, description, and SKILL.md path.",
            "inputSchema": {"type":"object","properties":{},"additionalProperties":false}
        }),
        json!({
            "name": "skill_search",
            "description": "Search installed WorkBuddy skills by name or description before choosing a workflow.",
            "inputSchema": {
                "type":"object",
                "properties":{"query":{"type":"string","minLength":1}},
                "required":["query"],
                "additionalProperties":false
            }
        }),
    ];
    tools.extend(office::tool_definitions());
    tools.extend(harvis::tool_definitions());
    tools.extend(im::tool_definitions());
    tools.extend(skills::tool_definitions());
    tools.extend(staffdeck::tool_definitions());
    tools
}

async fn call_tool(context: &HostContext, params: &Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tools/call requires name"))?;
    let arguments = params.get("arguments").unwrap_or(&Value::Null);
    match name {
        "memory_search" => {
            let query = required_string(arguments, "query")?;
            let owner_id = optional_string(arguments, "owner_id", "xiaoheng");
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(10)
                .clamp(1, 50);
            tool_success(&search_memories(context, &owner_id, &query, limit)?)
        }
        "memory_append" => {
            let content = required_string(arguments, "content")?;
            if content.chars().count() > 8000 {
                return Err(anyhow!("content exceeds 8000 characters"));
            }
            let owner_id = optional_string(arguments, "owner_id", "xiaoheng");
            let category = optional_string(arguments, "category", "general");
            tool_success(&append_memory(context, &owner_id, &category, &content)?)
        }
        "skill_list" => tool_success(&list_skills(context)?),
        "skill_search" => {
            let query = required_string(arguments, "query")?.to_lowercase();
            let matches = list_skills(context)?
                .into_iter()
                .filter(|skill| {
                    skill.name.to_lowercase().contains(&query)
                        || skill.description.to_lowercase().contains(&query)
                })
                .collect::<Vec<_>>();
            tool_success(&matches)
        }
        _ if office::handles(name) => office::call(name, arguments).await,
        _ if harvis::handles(name) => harvis::call(context, name, arguments).await,
        _ if im::handles(name) => im::call(context, name, arguments).await,
        _ if skills::handles(name) => skills::call(context, name, arguments).await,
        _ if staffdeck::handles(name) => staffdeck::call(context, name, arguments),
        _ => Err(anyhow!("Unknown WorkBuddy tool: {name}")),
    }
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

fn optional_string(arguments: &Value, key: &str, default: &str) -> String {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .to_string()
}

fn tool_success<T: Serialize>(value: &T) -> Result<Value> {
    let structured = serde_json::to_value(value)?;
    let text = serde_json::to_string_pretty(value)?;
    Ok(json!({
        "content":[{"type":"text","text":text}],
        "structuredContent": structured,
        "isError": false
    }))
}

fn tool_error(message: String) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}

fn open_database(context: &HostContext) -> Result<Connection> {
    ensure_parent(&context.database_path)?;
    let connection = Connection::open(&context.database_path)
        .with_context(|| format!("open {}", context.database_path.display()))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            owner_id TEXT NOT NULL,
            content TEXT NOT NULL,
            category TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_memories_owner_created
            ON memories(owner_id, created_at DESC);",
    )?;
    migrate_legacy_memories(context, &connection)?;
    Ok(connection)
}

fn migrate_legacy_memories(context: &HostContext, target: &Connection) -> Result<()> {
    let Some(path) = context
        .legacy_database_path
        .as_ref()
        .filter(|path| path.is_file())
    else {
        return Ok(());
    };
    if path == &context.database_path {
        return Ok(());
    }
    let legacy = Connection::open(path)?;
    let has_memories = legacy
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memories')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or_default()
        != 0;
    if !has_memories {
        return Ok(());
    }
    let columns = table_columns(&legacy, "memories")?;
    if !["id", "content", "category", "created_at"]
        .iter()
        .all(|column| columns.contains(*column))
    {
        return Ok(());
    }
    let owner_expression = if columns.contains("owner_id") {
        "COALESCE(owner_id, 'piscis')"
    } else {
        "'piscis'"
    };
    let mut statement = legacy.prepare(&format!(
        "SELECT id, content, category, {owner_expression}, created_at FROM memories"
    ))?;
    let records = statement.query_map([], |row| {
        let owner: String = row.get(3)?;
        Ok(MemoryRecord {
            id: row.get(0)?,
            content: row.get(1)?,
            category: row.get(2)?,
            owner_id: if owner == "piscis" {
                "xiaoheng".to_string()
            } else {
                owner
            },
            created_at: row.get(4)?,
        })
    })?;
    for record in records {
        let record = record?;
        target.execute(
            "INSERT OR IGNORE INTO memories (id, owner_id, content, category, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.id, record.owner_id, record.content, record.category, record.created_at],
        )?;
    }
    Ok(())
}

fn append_memory(
    context: &HostContext,
    owner_id: &str,
    category: &str,
    content: &str,
) -> Result<MemoryRecord> {
    let record = MemoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        owner_id: owner_id.to_string(),
        content: content.to_string(),
        category: category.to_string(),
        created_at: Utc::now().to_rfc3339(),
    };
    open_database(context)?.execute(
        "INSERT INTO memories (id, owner_id, content, category, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![record.id, record.owner_id, record.content, record.category, record.created_at],
    )?;
    append_legacy_memory(context, &record)?;
    Ok(record)
}

fn append_legacy_memory(context: &HostContext, record: &MemoryRecord) -> Result<()> {
    let Some(path) = context
        .legacy_database_path
        .as_ref()
        .filter(|path| path.is_file())
    else {
        return Ok(());
    };
    let legacy = Connection::open(path)?;
    let columns = table_columns(&legacy, "memories")?;
    if columns.is_empty()
        || !["id", "content", "category", "created_at"]
            .iter()
            .all(|column| columns.contains(*column))
    {
        return Ok(());
    }
    let candidates = [
        ("id", SqlValue::Text(record.id.clone())),
        ("content", SqlValue::Text(record.content.clone())),
        ("category", SqlValue::Text(record.category.clone())),
        ("confidence", SqlValue::Real(0.8)),
        ("owner_id", SqlValue::Text(record.owner_id.clone())),
        ("scope_type", SqlValue::Text("private".to_string())),
        ("scope_id", SqlValue::Text(record.owner_id.clone())),
        ("kind", SqlValue::Text("fact".to_string())),
        ("last_seen_at", SqlValue::Text(record.created_at.clone())),
        ("created_at", SqlValue::Text(record.created_at.clone())),
        ("updated_at", SqlValue::Text(record.created_at.clone())),
    ];
    let selected = candidates
        .into_iter()
        .filter(|(column, _)| columns.contains(*column))
        .collect::<Vec<_>>();
    let names = selected
        .iter()
        .map(|(column, _)| *column)
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = (1..=selected.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    legacy.execute(
        &format!("INSERT OR IGNORE INTO memories ({names}) VALUES ({placeholders})"),
        params_from_iter(selected.into_iter().map(|(_, value)| value)),
    )?;
    Ok(())
}

fn table_columns(connection: &Connection, table: &str) -> Result<BTreeSet<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    columns
        .collect::<rusqlite::Result<BTreeSet<_>>>()
        .map_err(Into::into)
}

fn search_memories(
    context: &HostContext,
    owner_id: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<MemoryRecord>> {
    let connection = open_database(context)?;
    let mut statement = connection.prepare(
        "SELECT id, owner_id, content, category, created_at FROM memories
         WHERE owner_id = ?1 AND (content LIKE ?2 OR category LIKE ?2)
         ORDER BY created_at DESC LIMIT ?3",
    )?;
    let pattern = format!("%{query}%");
    let records = statement.query_map(params![owner_id, pattern, limit], |row| {
        Ok(MemoryRecord {
            id: row.get(0)?,
            owner_id: row.get(1)?,
            content: row.get(2)?,
            category: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    records
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn list_skills(context: &HostContext) -> Result<Vec<SkillRecord>> {
    let mut skills = BTreeMap::new();
    for root in &context.skills_roots {
        collect_skill_files(root, root, 0, &mut skills)?;
    }
    Ok(skills.into_values().collect())
}

fn collect_skill_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    skills: &mut BTreeMap<String, SkillRecord>,
) -> Result<()> {
    if depth > 5 || !directory.is_dir() {
        return Ok(());
    }
    let skill_md = directory.join("SKILL.md");
    if skill_md.is_file() {
        let record = read_skill(root, &skill_md)?;
        skills.entry(record.name.clone()).or_insert(record);
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_skill_files(root, &path, depth + 1, skills)?;
        }
    }
    Ok(())
}

fn read_skill(root: &Path, skill_md: &Path) -> Result<SkillRecord> {
    let content = std::fs::read_to_string(skill_md)?;
    let frontmatter = content
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("---"))
        .map(|(yaml, _)| serde_yaml::from_str::<SkillFrontmatter>(yaml))
        .transpose()?
        .unwrap_or_default();
    let fallback_name = skill_md
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or("unknown-skill");
    let name = if frontmatter.name.trim().is_empty() {
        fallback_name.to_string()
    } else {
        frontmatter.name.trim().to_string()
    };
    Ok(SkillRecord {
        name,
        description: frontmatter.description.trim().to_string(),
        path: root
            .join(skill_md.strip_prefix(root).unwrap_or(skill_md))
            .display()
            .to_string(),
    })
}

pub fn context_from_environment() -> HostContext {
    let app_data = std::env::var_os("WORKBUDDY_APP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let database_path = std::env::var_os("WORKBUDDY_DATABASE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| app_data.join("workbuddy.db"));
    let legacy_database_path = Some(database_path.clone());
    let skills_roots = std::env::var_os("WORKBUDDY_SKILLS_DIRS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_else(|| vec![app_data.join("skills")]);
    let marketing_base_url = std::env::var("WORKBUDDY_MARKETING_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://61.150.31.150:8090".to_string());
    HostContext::new(
        app_data,
        database_path,
        legacy_database_path,
        skills_roots,
        marketing_base_url,
    )
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> (HostContext, PathBuf) {
        let root = std::env::temp_dir().join(format!("workbuddy-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create test root");
        (
            HostContext::new(
                root.clone(),
                root.join("workbuddy.db"),
                None,
                vec![root.join("skills")],
                "http://127.0.0.1:9".to_string(),
            ),
            root,
        )
    }

    fn request(id: i64, method: &str, params: Value) -> Value {
        json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
    }

    #[tokio::test]
    async fn initializes_and_lists_workbuddy_domain_tools() {
        let (context, root) = test_context();
        let initialized = handle_request(
            &context,
            &request(1, "initialize", json!({"protocolVersion":"2025-06-18"})),
        )
        .await
        .expect("initialize response");
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "workbuddy-mcp-host"
        );
        let listed = handle_request(&context, &request(2, "tools/list", json!({})))
            .await
            .expect("tools response");
        let names = listed["result"]["tools"]
            .as_array()
            .expect("tool list")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"memory_search"));
        assert!(names.contains(&"memory_append"));
        assert!(names.contains(&"skill_list"));
        assert!(names.contains(&"skill_search"));
        assert!(names.contains(&"document_artifact"));
        assert!(names.contains(&"presentation_artifact"));
        assert!(names.contains(&"spreadsheet_artifact"));
        assert!(names.contains(&"harvis_image_generate"));
        assert!(names.contains(&"harvis_music_generate"));
        assert!(names.contains(&"harvis_job_status"));
        assert!(names.contains(&"harvis_save_result"));
        assert!(names.contains(&"skill_market_search"));
        assert!(names.contains(&"skill_install"));
        assert!(names.contains(&"skill_set_enabled"));
        assert!(names.contains(&"skill_uninstall"));
        assert!(names.contains(&"staff_list"));
        assert!(names.contains(&"staff_task_create"));
        assert!(names.contains(&"staff_task_update"));
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn appends_and_searches_memory_without_piscis_store() {
        let (context, root) = test_context();
        let appended = handle_request(&context, &request(1, "tools/call", json!({"name":"memory_append","arguments":{"content":"用户偏好简洁汇报","category":"preference"}}))).await.expect("append response");
        assert_eq!(appended["result"]["isError"], false);
        let searched = handle_request(
            &context,
            &request(
                2,
                "tools/call",
                json!({"name":"memory_search","arguments":{"query":"简洁","limit":5}}),
            ),
        )
        .await
        .expect("search response");
        assert!(searched["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("用户偏好简洁汇报"));
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn lists_skills_from_unified_skill_roots() {
        let (context, root) = test_context();
        let skill_dir = root.join("skills").join("harvis-creative");
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: harvis-creative\ndescription: 生成图片和音乐\n---\n# Harvis\n",
        )
        .expect("write skill");
        let listed = handle_request(
            &context,
            &request(1, "tools/call", json!({"name":"skill_list","arguments":{}})),
        )
        .await
        .expect("skill list response");
        assert!(listed["result"]["content"][0]["text"]
            .as_str()
            .expect("text")
            .contains("harvis-creative"));
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn migrates_legacy_memories_and_preserves_xiaoheng_identity() {
        let (mut context, root) = test_context();
        let legacy_path = root.join("piscis.db");
        let legacy = Connection::open(&legacy_path).expect("legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE memories (
                id TEXT PRIMARY KEY, content TEXT NOT NULL, category TEXT NOT NULL,
                confidence REAL, source_session_id TEXT, owner_id TEXT,
                scope_type TEXT, scope_id TEXT, project_scope_id TEXT, kind TEXT,
                evidence_session_id TEXT, evidence_tool_use_id TEXT, last_seen_at TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            INSERT INTO memories VALUES (
                'legacy-1','用户偏好简洁汇报','preference',0.9,NULL,'piscis',
                'private','piscis',NULL,'preference',NULL,NULL,'2026-08-25T00:00:00Z',
                '2026-08-25T00:00:00Z','2026-08-25T00:00:00Z'
            );",
            )
            .expect("legacy schema");
        drop(legacy);
        context.legacy_database_path = Some(legacy_path);

        let searched = handle_request(
            &context,
            &request(
                1,
                "tools/call",
                json!({"name":"memory_search","arguments":{"query":"简洁","owner_id":"xiaoheng"}}),
            ),
        )
        .await
        .expect("search response");
        assert!(searched["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("用户偏好简洁汇报"));
        std::fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn migrates_and_dual_writes_minimal_legacy_memory_schema() {
        let (mut context, root) = test_context();
        let legacy_path = root.join("piscis.db");
        let legacy = Connection::open(&legacy_path).expect("legacy database");
        legacy
            .execute_batch(
                "CREATE TABLE memories (
                    id TEXT PRIMARY KEY,
                    content TEXT NOT NULL,
                    category TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                INSERT INTO memories VALUES (
                    'legacy-minimal','早期版本记忆','fact','2026-06-23T00:00:00Z'
                );",
            )
            .expect("minimal legacy schema");
        drop(legacy);
        context.legacy_database_path = Some(legacy_path.clone());

        let searched = handle_request(
            &context,
            &request(
                1,
                "tools/call",
                json!({"name":"memory_search","arguments":{"query":"早期版本","owner_id":"xiaoheng"}}),
            ),
        )
        .await
        .expect("search response");
        assert!(searched["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("早期版本记忆"));

        let appended = handle_request(
            &context,
            &request(
                2,
                "tools/call",
                json!({"name":"memory_append","arguments":{"content":"升级后记忆","category":"fact"}}),
            ),
        )
        .await
        .expect("append response");
        assert_eq!(appended["result"]["isError"], false);
        let legacy = Connection::open(legacy_path).expect("reopen legacy database");
        let count: i64 = legacy
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE content='升级后记忆'",
                [],
                |row| row.get(0),
            )
            .expect("dual write count");
        assert_eq!(count, 1);
        std::fs::remove_dir_all(root).expect("remove test root");
    }
}
