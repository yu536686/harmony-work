use super::Database;
use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsageBudget {
    pub session_tokens: u64,
    pub project_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageOverview {
    pub workspace_root: String,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub project_tokens: u64,
    pub session_count: usize,
    pub budget: UsageBudget,
}

fn workspace_key(path: &str) -> String {
    let normalized = Path::new(path).canonicalize().ok()
        .map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    #[cfg(windows)]
    { normalized.replace('/', "\\").trim_end_matches('\\').to_lowercase() }
    #[cfg(not(windows))]
    { normalized.trim_end_matches('/').to_string() }
}

impl Database {
    fn ensure_usage_tables(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS usage_budgets (
            workspace TEXT PRIMARY KEY, session_tokens INTEGER NOT NULL, project_tokens INTEGER NOT NULL
        ); CREATE TABLE IF NOT EXISTS codex_usage_watermarks (
            session_id TEXT NOT NULL, thread_id TEXT NOT NULL, input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL, PRIMARY KEY(session_id, thread_id)
        );")?;
        Ok(())
    }

    /// Codex reports cumulative thread totals. Persist a watermark so replayed
    /// events and process restarts cannot double-count prior turns.
    pub fn record_codex_usage(&self, session_id: &str, thread_id: &str, input: u32, output: u32) -> Result<()> {
        self.ensure_usage_tables()?;
        let tx = self.conn.unchecked_transaction()?;
        let previous: (u32, u32) = tx.query_row(
            "SELECT input_tokens, output_tokens FROM codex_usage_watermarks WHERE session_id=?1 AND thread_id=?2",
            params![session_id, thread_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?.unwrap_or_default();
        tx.execute("UPDATE sessions SET total_input_tokens=total_input_tokens+?1,
            total_output_tokens=total_output_tokens+?2 WHERE id=?3",
            params![i64::from(input.saturating_sub(previous.0)), i64::from(output.saturating_sub(previous.1)), session_id])?;
        tx.execute("INSERT INTO codex_usage_watermarks VALUES (?1,?2,?3,?4)
            ON CONFLICT(session_id,thread_id) DO UPDATE SET input_tokens=MAX(input_tokens,excluded.input_tokens),
            output_tokens=MAX(output_tokens,excluded.output_tokens)",
            params![session_id, thread_id, input, output])?;
        tx.commit()?;
        Ok(())
    }

    pub fn usage_overview(&self, session_id: &str, default_workspace: &str) -> Result<UsageOverview> {
        self.ensure_usage_tables()?;
        let session = self.get_session(session_id)?.ok_or_else(|| anyhow!("会话不存在"))?;
        let root = session.workspace_root.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or(default_workspace);
        let key = workspace_key(root);
        let budget = self.conn.query_row("SELECT session_tokens,project_tokens FROM usage_budgets WHERE workspace=?1",
            [&key], |row| Ok(UsageBudget { session_tokens: row.get(0)?, project_tokens: row.get(1)? })).optional()?.unwrap_or_default();
        let mut statement = self.conn.prepare("SELECT workspace_root,total_input_tokens,total_output_tokens FROM sessions")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)))?;
        let mut total = 0_u64;
        let mut count = 0;
        for row in rows {
            let (workspace, input, output) = row?;
            let workspace = workspace.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or(default_workspace);
            if workspace_key(workspace) == key {
                total = total.saturating_add(input.max(0) as u64).saturating_add(output.max(0) as u64);
                count += 1;
            }
        }
        Ok(UsageOverview { workspace_root: root.to_string(), session_input_tokens: session.total_input_tokens.max(0) as u64,
            session_output_tokens: session.total_output_tokens.max(0) as u64, project_tokens: total, session_count: count, budget })
    }

    pub fn set_usage_budget(&self, session_id: &str, default_workspace: &str, budget: UsageBudget) -> Result<UsageOverview> {
        if budget.session_tokens > 1_000_000_000_000 || budget.project_tokens > 1_000_000_000_000 {
            return Err(anyhow!("Token 限额超出支持范围"));
        }
        let current = self.usage_overview(session_id, default_workspace)?;
        self.conn.execute("INSERT INTO usage_budgets VALUES (?1,?2,?3) ON CONFLICT(workspace) DO UPDATE SET
            session_tokens=excluded.session_tokens,project_tokens=excluded.project_tokens",
            params![workspace_key(&current.workspace_root), budget.session_tokens as i64, budget.project_tokens as i64])?;
        self.usage_overview(session_id, default_workspace)
    }
}

impl UsageOverview {
    pub fn check(&self) -> Result<()> {
        let session_total = self.session_input_tokens.saturating_add(self.session_output_tokens);
        if self.budget.session_tokens > 0 && session_total >= self.budget.session_tokens {
            return Err(anyhow!("已达到此会话 Token 限额，请在工具面板的用量中调整限额后再提交。"));
        }
        if self.budget.project_tokens > 0 && self.project_tokens >= self.budget.project_tokens {
            return Err(anyhow!("已达到项目 Token 限额，请在工具面板的用量中调整限额后再提交。"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_usage_is_idempotent_and_budget_is_project_scoped() {
        let db = Database::open_in_memory().unwrap();
        let a = db.create_session(Some("A")).unwrap();
        let b = db.create_session(Some("B")).unwrap();
        db.record_codex_usage(&a.id, "thread-a", 80, 20).unwrap();
        db.record_codex_usage(&a.id, "thread-a", 80, 20).unwrap();
        db.record_codex_usage(&a.id, "thread-a", 40, 10).unwrap();
        assert_eq!(db.usage_overview(&a.id, "/project").unwrap().project_tokens, 100);
        db.record_codex_usage(&a.id, "thread-b", 10, 5).unwrap();
        db.set_usage_budget(&a.id, "/project", UsageBudget { session_tokens: 0, project_tokens: 110 }).unwrap();
        assert!(db.usage_overview(&b.id, "/project").unwrap().check().is_err());
        db.set_usage_budget(&a.id, "/project", UsageBudget::default()).unwrap();
        assert!(db.usage_overview(&a.id, "/project").unwrap().check().is_ok());
        assert_eq!(db.usage_overview(&a.id, "/project").unwrap().project_tokens, 115);
        assert_eq!(db.usage_overview(&b.id, "/other-project").unwrap().project_tokens, 115);
    }
}
