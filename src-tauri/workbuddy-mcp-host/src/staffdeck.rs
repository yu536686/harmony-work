use crate::HostContext;
use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(crate) fn handles(name: &str) -> bool {
    matches!(
        name,
        "staff_list"
            | "staff_status_set"
            | "staff_project_list"
            | "staff_task_list"
            | "staff_task_create"
            | "staff_task_update"
    )
}

pub(crate) fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name":"staff_list","description":"List StaffDeck personnel with role, description, execution prompt, model assignment, and live work status.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
            "annotations":{"title":"人员列表","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true}
        }),
        json!({
            "name":"staff_status_set","description":"Update one StaffDeck person's work status so the personnel card reflects idle, busy, paused, or error state.",
            "inputSchema":{"type":"object","properties":{"staff_id":{"type":"string"},"status":{"type":"string","enum":["idle","busy","paused","error"]}},"required":["staff_id","status"],"additionalProperties":false},
            "annotations":{"title":"更新人员状态","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true}
        }),
        json!({
            "name":"staff_project_list","description":"List StaffDeck projects and their workspace directories.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false},
            "annotations":{"title":"项目列表","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true}
        }),
        json!({
            "name":"staff_task_list","description":"List StaffDeck Todo items, optionally filtered by staff member, project, or status.",
            "inputSchema":{"type":"object","properties":{"staff_id":{"type":"string"},"project_id":{"type":"string"},"status":{"type":"string"}},"additionalProperties":false},
            "annotations":{"title":"人员任务列表","readOnlyHint":true,"destructiveHint":false,"idempotentHint":true}
        }),
        json!({
            "name":"staff_task_create","description":"Create and assign one concrete StaffDeck Todo. This updates the shared task board; execution is performed by the Codex staff dispatcher.",
            "inputSchema":{"type":"object","properties":{
                "staff_id":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},
                "priority":{"type":"string","enum":["low","medium","high","urgent"],"default":"medium"},
                "project_id":{"type":"string"},"depends_on":{"type":"string"},"task_timeout_secs":{"type":"integer","minimum":0,"maximum":86400}
            },"required":["staff_id","title"],"additionalProperties":false},
            "annotations":{"title":"分配人员任务","readOnlyHint":false,"destructiveHint":false,"idempotentHint":false}
        }),
        json!({
            "name":"staff_task_update","description":"Update one StaffDeck Todo's title, description, priority, status, or blocked reason.",
            "inputSchema":{"type":"object","properties":{
                "task_id":{"type":"string"},"title":{"type":"string"},"description":{"type":"string"},
                "priority":{"type":"string","enum":["low","medium","high","urgent"]},
                "status":{"type":"string","enum":["todo","in_progress","blocked","needs_review","done","cancelled"]},
                "blocked_reason":{"type":"string"}
            },"required":["task_id"],"additionalProperties":false},
            "annotations":{"title":"更新人员任务","readOnlyHint":false,"destructiveHint":false,"idempotentHint":true}
        }),
    ]
}

pub(crate) fn call(context: &HostContext, name: &str, arguments: &Value) -> Result<Value> {
    let database = legacy_database(context)?;
    match name {
        "staff_list" => mcp_success(list_staff(&database)?),
        "staff_status_set" => mcp_success(set_staff_status(&database, arguments)?),
        "staff_project_list" => mcp_success(list_projects(&database)?),
        "staff_task_list" => mcp_success(list_tasks(&database, arguments)?),
        "staff_task_create" => mcp_success(create_task(&database, arguments)?),
        "staff_task_update" => mcp_success(update_task(&database, arguments)?),
        _ => Err(anyhow!("unknown StaffDeck tool: {name}")),
    }
}

fn legacy_database(context: &HostContext) -> Result<Connection> {
    let path = context
        .legacy_database_path
        .as_ref()
        .filter(|path| path.is_file())
        .ok_or_else(|| anyhow!("StaffDeck database has not been migrated or initialized"))?;
    Ok(Connection::open(path)?)
}

fn list_staff(database: &Connection) -> Result<Value> {
    let mut statement = database.prepare(
        "SELECT id,name,role,icon,color,system_prompt,description,status,llm_provider_id,max_iterations,task_timeout_secs,created_at,updated_at FROM kois ORDER BY created_at ASC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(json!({
            "id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"role":row.get::<_,String>(2).unwrap_or_default(),
            "icon":row.get::<_,String>(3).unwrap_or_default(),"color":row.get::<_,String>(4).unwrap_or_default(),
            "systemPrompt":row.get::<_,String>(5).unwrap_or_default(),"description":row.get::<_,String>(6).unwrap_or_default(),
            "status":row.get::<_,String>(7).unwrap_or_else(|_| "idle".to_string()),"llmProviderId":row.get::<_,Option<String>>(8).unwrap_or(None),
            "maxIterations":row.get::<_,i64>(9).unwrap_or_default(),"taskTimeoutSecs":row.get::<_,i64>(10).unwrap_or_default(),
            "createdAt":row.get::<_,String>(11).unwrap_or_default(),"updatedAt":row.get::<_,String>(12).unwrap_or_default()
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<Vec<_>>>()?))
}

fn set_staff_status(database: &Connection, input: &Value) -> Result<Value> {
    let id = required_string(input, "staff_id")?;
    let status = required_string(input, "status")?;
    if !matches!(status.as_str(), "idle" | "busy" | "paused" | "error") {
        return Err(anyhow!("invalid staff status"));
    }
    let changed = database.execute(
        "UPDATE kois SET status=?1,updated_at=?2 WHERE id=?3",
        params![status, chrono::Utc::now().to_rfc3339(), id],
    )?;
    if changed == 0 {
        return Err(anyhow!("staff member not found"));
    }
    Ok(json!({"ok":true,"staffId":id,"status":status}))
}

fn list_projects(database: &Connection) -> Result<Value> {
    let mut statement = database.prepare(
        "SELECT id,name,org_spec,status,project_dir,task_timeout_secs,created_at,updated_at FROM pool_sessions ORDER BY updated_at DESC",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(json!({
            "id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"orgSpec":row.get::<_,String>(2).unwrap_or_default(),
            "status":row.get::<_,String>(3).unwrap_or_else(|_| "active".to_string()),"projectDir":row.get::<_,Option<String>>(4).unwrap_or(None),
            "taskTimeoutSecs":row.get::<_,i64>(5).unwrap_or_default(),"createdAt":row.get::<_,String>(6).unwrap_or_default(),
            "updatedAt":row.get::<_,String>(7).unwrap_or_default()
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<Vec<_>>>()?))
}

fn list_tasks(database: &Connection, input: &Value) -> Result<Value> {
    let staff_id = optional_string(input, "staff_id");
    let project_id = optional_string(input, "project_id");
    let status = optional_string(input, "status");
    let mut statement = database.prepare(
        "SELECT id,owner_id,title,description,status,priority,assigned_by,pool_session_id,claimed_by,claimed_at,depends_on,blocked_reason,result_message_id,source_type,task_timeout_secs,git_branch,integration_status,created_at,updated_at FROM koi_todos WHERE (?1 IS NULL OR owner_id=?1) AND (?2 IS NULL OR pool_session_id=?2) AND (?3 IS NULL OR status=?3) ORDER BY created_at DESC",
    )?;
    let rows = statement.query_map(params![staff_id, project_id, status], task_row)?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<Vec<_>>>()?))
}

fn task_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id":row.get::<_,String>(0)?,"ownerId":row.get::<_,String>(1)?,"title":row.get::<_,String>(2)?,
        "description":row.get::<_,String>(3).unwrap_or_default(),"status":row.get::<_,String>(4)?,"priority":row.get::<_,String>(5)?,
        "assignedBy":row.get::<_,String>(6).unwrap_or_default(),"projectId":row.get::<_,Option<String>>(7).unwrap_or(None),
        "claimedBy":row.get::<_,Option<String>>(8).unwrap_or(None),"claimedAt":row.get::<_,Option<String>>(9).unwrap_or(None),
        "dependsOn":row.get::<_,Option<String>>(10).unwrap_or(None),"blockedReason":row.get::<_,Option<String>>(11).unwrap_or(None),
        "resultMessageId":row.get::<_,Option<i64>>(12).unwrap_or(None),"sourceType":row.get::<_,String>(13).unwrap_or_else(|_| "user".to_string()),
        "taskTimeoutSecs":row.get::<_,i64>(14).unwrap_or_default(),"gitBranch":row.get::<_,Option<String>>(15).unwrap_or(None),
        "integrationStatus":row.get::<_,Option<String>>(16).unwrap_or(None),"createdAt":row.get::<_,String>(17).unwrap_or_default(),
        "updatedAt":row.get::<_,String>(18).unwrap_or_default()
    }))
}

fn create_task(database: &Connection, input: &Value) -> Result<Value> {
    let owner_id = required_string(input, "staff_id")?;
    let exists = database.query_row(
        "SELECT EXISTS(SELECT 1 FROM kois WHERE id=?1)",
        params![owner_id],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if !exists {
        return Err(anyhow!("staff member not found"));
    }
    let title = required_string(input, "title")?;
    let description = optional_string(input, "description").unwrap_or_default();
    let priority = optional_string(input, "priority").unwrap_or_else(|| "medium".to_string());
    let project_id = optional_string(input, "project_id");
    if let Some(project_id) = project_id.as_deref() {
        let project_exists = database.query_row(
            "SELECT EXISTS(SELECT 1 FROM pool_sessions WHERE id=?1)",
            params![project_id],
            |row| row.get::<_, i64>(0),
        )? != 0;
        if !project_exists {
            return Err(anyhow!("StaffDeck project not found"));
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    database.execute(
        "INSERT INTO koi_todos (id,owner_id,title,description,status,priority,assigned_by,pool_session_id,source_type,depends_on,task_timeout_secs,created_at,updated_at) VALUES (?1,?2,?3,?4,'todo',?5,'xiaoheng',?6,'codex',?7,?8,?9,?9)",
        params![id, owner_id, title, description, priority, project_id, optional_string(input,"depends_on"), input.get("task_timeout_secs").and_then(Value::as_u64).unwrap_or_default() as i64, now],
    )?;
    database.query_row(
        "SELECT id,owner_id,title,description,status,priority,assigned_by,pool_session_id,claimed_by,claimed_at,depends_on,blocked_reason,result_message_id,source_type,task_timeout_secs,git_branch,integration_status,created_at,updated_at FROM koi_todos WHERE id=?1",
        params![id], task_row,
    ).map_err(Into::into)
}

fn update_task(database: &Connection, input: &Value) -> Result<Value> {
    let id = required_string(input, "task_id")?;
    let status = optional_string(input, "status");
    let blocked_reason = optional_string(input, "blocked_reason");
    let changed = database.execute(
        "UPDATE koi_todos SET title=COALESCE(?2,title),description=COALESCE(?3,description),priority=COALESCE(?4,priority),status=COALESCE(?5,status),blocked_reason=CASE WHEN COALESCE(?5,status) IN ('blocked','needs_review') THEN COALESCE(?6,blocked_reason) ELSE NULL END,updated_at=?7 WHERE id=?1",
        params![id, optional_string(input,"title"), optional_string(input,"description"), optional_string(input,"priority"), status, blocked_reason, chrono::Utc::now().to_rfc3339()],
    )?;
    if changed == 0 {
        return Err(anyhow!("StaffDeck task not found"));
    }
    database.query_row(
        "SELECT id,owner_id,title,description,status,priority,assigned_by,pool_session_id,claimed_by,claimed_at,depends_on,blocked_reason,result_message_id,source_type,task_timeout_secs,git_branch,integration_status,created_at,updated_at FROM koi_todos WHERE id=?1",
        params![id], task_row,
    ).map_err(Into::into)
}

fn required_string(input: &Value, key: &str) -> Result<String> {
    optional_string(input, key).ok_or_else(|| anyhow!("{key} is required"))
}

fn optional_string(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
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
    fn creates_and_lists_staff_task_without_piscis_types() {
        let root = std::env::temp_dir().join(format!("staffdeck-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Connection::open(root.join("legacy.db")).unwrap();
        database.execute_batch(
            "CREATE TABLE kois (id TEXT PRIMARY KEY,name TEXT,role TEXT,icon TEXT,color TEXT,system_prompt TEXT,description TEXT,status TEXT,created_at TEXT,updated_at TEXT,llm_provider_id TEXT,max_iterations INTEGER,task_timeout_secs INTEGER);
             CREATE TABLE pool_sessions (id TEXT PRIMARY KEY,name TEXT,org_spec TEXT,status TEXT,project_dir TEXT,task_timeout_secs INTEGER,created_at TEXT,updated_at TEXT);
             CREATE TABLE koi_todos (id TEXT PRIMARY KEY,owner_id TEXT,title TEXT,description TEXT,status TEXT,priority TEXT,assigned_by TEXT,pool_session_id TEXT,claimed_by TEXT,claimed_at TEXT,depends_on TEXT,blocked_reason TEXT,result_message_id INTEGER,source_type TEXT,task_timeout_secs INTEGER,git_branch TEXT,integration_status TEXT,created_at TEXT,updated_at TEXT);
             INSERT INTO kois VALUES ('pm','产品经理','产品','','','','','idle','now','now',NULL,0,0);",
        ).unwrap();
        let task =
            create_task(&database, &json!({"staff_id":"pm","title":"职责介绍 PPT"})).unwrap();
        assert_eq!(task["ownerId"], "pm");
        let tasks = list_tasks(&database, &json!({"staff_id":"pm"})).unwrap();
        assert_eq!(tasks.as_array().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
