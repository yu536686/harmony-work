//! `plan_todo` — kernel-neutral implementation of the visible execution
//! plan tool.
//!
//! The desktop implementation lived in `src-tauri/src/tools/plan_todo.rs`
//! and emitted a Tauri event directly via `AppHandle::emit`. This
//! kernel version writes into a shared [`PlanStore`] and forwards the
//! resulting [`AgentEvent::PlanUpdate`] through a host-supplied
//! [`EventSink`], which the desktop host bridges back to Tauri in a
//! separate adapter.

use crate::agent::messages::AgentEvent;
use crate::agent::plan::{
    merge_todos, plans_equal, summarize_todos, validate_todos, PlanStore, PlanTodoItem,
};
use crate::agent::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use piscis_core::host::EventSink;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct PlanTodoTool {
    pub store: PlanStore,
    pub event_sink: Arc<dyn EventSink>,
}

#[async_trait]
impl Tool for PlanTodoTool {
    fn name(&self) -> &str {
        "plan_todo"
    }

    fn description(&self) -> &str {
        "Maintain a short visible execution plan for the current task. \
         Use this for non-trivial tasks with multiple steps so the user can see what you plan to do and what is currently in progress. \
         Keep plans concise (usually 2-7 items), update statuses as you work, and make sure at most one item is 'in_progress' at a time. \
         Prefer replacing the whole list when your plan changes significantly; use merge=true for incremental updates. \
         This tool only updates your visible plan. It does NOT execute work, does NOT create deliverables, does NOT send pool messages, and does NOT replace using the real tools needed to move the task forward."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["todos"],
            "properties": {
                "merge": {
                    "type": "boolean",
                    "description": "If true, update existing todo items by id and append new ones. If false or omitted, replace the whole plan."
                },
                "todos": {
                    "type": "array",
                    "description": "The plan items to set or update.",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "required": ["id", "content", "status"],
                        "properties": {
                            "id": { "type": "string", "description": "Stable todo id, e.g. 'scan-files'" },
                            "content": { "type": "string", "description": "Short user-facing task description" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            }
                        }
                    }
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let merge = input["merge"].as_bool().unwrap_or(false);
        let todos: Vec<PlanTodoItem> = match serde_json::from_value(input["todos"].clone()) {
            Ok(items) => items,
            Err(e) => return Ok(ToolResult::err(format!("'todos' 格式无效: {}", e))),
        };
        // Canonicalise before comparing or storing. Models often resend the
        // same board with incidental whitespace changes; that is not progress
        // and must not create another UI event or another loop turn.
        let todos: Vec<PlanTodoItem> = todos.iter().map(PlanTodoItem::normalized).collect();
        if let Err(e) = validate_todos(&todos) {
            return Ok(ToolResult::err(e));
        }

        let (updated, changed) = {
            let mut state = self.store.lock().await;
            let current = state.get(&ctx.session_id).cloned().unwrap_or_default();
            let next: Vec<PlanTodoItem> = if merge {
                merge_todos(&current, &todos)
            } else {
                todos.clone()
            }
            .iter()
            .map(PlanTodoItem::normalized)
            .collect();
            if let Err(e) = validate_todos(&next) {
                return Ok(ToolResult::err(e));
            }
            if plans_equal(&current, &next) {
                (current, false)
            } else {
                state.insert(ctx.session_id.clone(), next.clone());
                (next, true)
            }
        };

        if !changed {
            return Ok(ToolResult::ok(format!(
                "计划未变化（{} 项）。请直接执行下一项实际工作，除非状态或内容确实发生变化。",
                updated.len()
            )));
        }

        let payload = serde_json::to_value(AgentEvent::PlanUpdate {
            items: updated.clone(),
        })
        .unwrap_or(Value::Null);
        self.event_sink
            .emit_session(&ctx.session_id, "agent_event", payload);

        let scope_note = if ctx.pool_session_id.is_some() {
            "\n\n注意：这只更新你的内部计划板，不会把结果发送到 `pool_chat`，也不会创建、认领或完成 `pool_org` 的协作 todo。\
             如果你在协作池中工作，更新计划后仍需要用实际执行工具推进交付，并在需要时显式 handoff 或汇报状态。"
        } else {
            "\n\n注意：这只更新计划板，不会直接完成任务。更新计划后仍需要调用实际工具或直接产出结果。"
        };

        Ok(ToolResult::ok(format!(
            "计划已更新（{} 项）:\n{}{}\n\n现在直接执行当前步骤；在取得真实工具结果前，不要再次调用 plan_todo。",
            updated.len(),
            summarize_todos(&updated),
            scope_note
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::PlanTodoTool;
    use crate::agent::plan::{new_plan_store, PlanTodoItem};
    use crate::agent::tool::{Tool, ToolContext, ToolSettings};
    use piscis_core::host::EventSink;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::{atomic::AtomicBool, Arc, Mutex};

    #[derive(Default)]
    struct TestSink {
        session_events: Mutex<Vec<(String, String)>>,
    }

    impl EventSink for TestSink {
        fn emit_session(&self, session_id: &str, event: &str, _payload: Value) {
            self.session_events
                .lock()
                .unwrap()
                .push((session_id.to_string(), event.to_string()));
        }

        fn emit_broadcast(&self, _event: &str, _payload: Value) {}
    }

    fn context() -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            workspace_root: PathBuf::from("."),
            bypass_permissions: false,
            settings: Arc::new(ToolSettings::default()),
            max_iterations: None,
            memory_owner_id: "piscis".to_string(),
            pool_session_id: None,
            tool_use_id: None,
            cancel: Arc::new(AtomicBool::new(false)),
            loop_halt: None,
        }
    }

    #[tokio::test]
    async fn unchanged_plan_is_a_noop_and_emits_no_second_event() {
        let store = new_plan_store();
        store.lock().await.insert(
            "test-session".to_string(),
            vec![PlanTodoItem {
                id: "inspect".to_string(),
                content: "读取文件".to_string(),
                status: "in_progress".to_string(),
            }],
        );
        let sink = Arc::new(TestSink::default());
        let tool = PlanTodoTool {
            store: store.clone(),
            event_sink: sink.clone(),
        };

        let result = tool
            .call(
                json!({"todos": [{"id": " inspect ", "content": " 读取文件 ", "status": " in_progress "}]}),
                &context(),
            )
            .await
            .expect("tool call should succeed");

        assert!(!result.is_error);
        assert!(result.content.contains("计划未变化"));
        assert!(sink.session_events.lock().unwrap().is_empty());
        assert_eq!(store.lock().await["test-session"][0].id, "inspect");
    }
}
