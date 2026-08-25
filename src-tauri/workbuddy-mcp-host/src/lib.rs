use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod harvis;
mod office;

#[derive(Debug, Clone)]
pub struct HostContext {
    pub app_data_dir: PathBuf,
    pub database_path: PathBuf,
    pub skills_roots: Vec<PathBuf>,
    pub marketing_base_url: String,
}

impl HostContext {
    pub fn new(
        app_data_dir: PathBuf,
        database_path: PathBuf,
        skills_roots: Vec<PathBuf>,
        marketing_base_url: String,
    ) -> Self {
        Self {
            app_data_dir,
            database_path,
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
    Ok(connection)
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
    Ok(record)
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
    let skills_roots = std::env::var_os("WORKBUDDY_SKILLS_DIRS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_else(|| vec![app_data.join("skills")]);
    let marketing_base_url = std::env::var("WORKBUDDY_MARKETING_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://61.150.31.150:8090".to_string());
    HostContext::new(app_data, database_path, skills_roots, marketing_base_url)
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
}
