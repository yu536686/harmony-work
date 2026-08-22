use anyhow::Result;
use async_trait::async_trait;
use piscis_kernel::agent::tool::{
    Tool, ToolContext, ToolExecutionMode, ToolExposure, ToolMetadata, ToolResult, ToolRisk,
};
use serde_json::{json, Map, Value};

pub struct ComputerUseTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendRoute {
    ObserveWindows,
    Screen(&'static str),
    Uia(&'static str),
    Desktop(&'static str),
}

fn route_action(
    action: &str,
    has_selector: bool,
    is_windows: bool,
) -> std::result::Result<BackendRoute, String> {
    let route = match action {
        "observe" if is_windows => BackendRoute::ObserveWindows,
        "observe" => BackendRoute::Screen("capture"),
        "list_windows" if is_windows => BackendRoute::Uia("list_windows"),
        "list_windows" => BackendRoute::Desktop("list_windows"),
        "click" if is_windows && has_selector => BackendRoute::Uia("click"),
        "click" => BackendRoute::Desktop("click"),
        "type_text" if is_windows && has_selector => BackendRoute::Uia("type"),
        "type_text" => BackendRoute::Desktop("type_text"),
        "press_key" if is_windows => BackendRoute::Uia("send_hotkey"),
        "press_key" => BackendRoute::Desktop("hotkey"),
        "scroll" if is_windows && has_selector => BackendRoute::Uia("scroll"),
        "scroll" => BackendRoute::Desktop("scroll"),
        "drag" => BackendRoute::Desktop("drag"),
        "wait_for_element" if is_windows => BackendRoute::Uia("wait_for_element"),
        "activate_window" if is_windows => BackendRoute::Uia("activate_window"),
        "activate_window" => BackendRoute::Desktop("activate_window"),
        "launch_app" => BackendRoute::Desktop("launch_app"),
        _ => return Err(format!("Unsupported computer_use action: {action}")),
    };
    Ok(route)
}

fn has_selector(input: &Value) -> bool {
    ["name", "automation_id", "control_type", "class_name"]
        .iter()
        .any(|key| {
            input
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
        })
}

fn copy_if_present(source: &Value, target: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        if let Some(value) = source.get(*key) {
            target.insert((*key).to_string(), value.clone());
        }
    }
}

#[cfg(target_os = "windows")]
fn uia_input(input: &Value, action: &str) -> Value {
    let mut mapped = Map::new();
    mapped.insert("action".into(), json!(action));
    copy_if_present(
        input,
        &mut mapped,
        &[
            "window_title",
            "name",
            "automation_id",
            "control_type",
            "class_name",
            "text",
            "x",
            "y",
            "direction",
            "amount",
            "timeout_ms",
            "max_elements",
        ],
    );
    if action == "send_hotkey" {
        if let Some(key) = input.get("key") {
            mapped.insert("hotkey".into(), key.clone());
        }
    }
    Value::Object(mapped)
}

fn desktop_input(input: &Value, action: &str) -> Value {
    let mut mapped = Map::new();
    mapped.insert("action".into(), json!(action));
    copy_if_present(
        input,
        &mut mapped,
        &["window_title", "app_name", "text", "x", "y"],
    );
    if action == "drag" {
        if let Some(value) = input.get("x2") {
            mapped.insert("to_x".into(), value.clone());
        }
        if let Some(value) = input.get("y2") {
            mapped.insert("to_y".into(), value.clone());
        }
    }
    if action == "hotkey" {
        let keys = input
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        mapped.insert("keys".into(), json!(keys));
    }
    if action == "scroll" {
        if let Some(value) = input.get("direction") {
            mapped.insert("scroll_direction".into(), value.clone());
        }
        if let Some(value) = input.get("amount") {
            mapped.insert("scroll_amount".into(), value.clone());
        }
    }
    Value::Object(mapped)
}

fn screen_input(input: &Value, action: &str) -> Value {
    let mut mapped = Map::new();
    mapped.insert("action".into(), json!(action));
    mapped.insert("format".into(), json!("png"));
    copy_if_present(
        input,
        &mut mapped,
        &["window_title", "grid", "grid_spacing", "output_path"],
    );
    Value::Object(mapped)
}

#[async_trait]
impl Tool for ComputerUseTool {
    fn name(&self) -> &str {
        "computer_use"
    }

    fn description(&self) -> &str {
        "Observe and control local desktop applications through one stable interface. On Windows, semantic selectors use UI Automation; screenshots and coordinate actions use cross-platform backends. Call observe before acting when the current UI state is unknown."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["observe", "list_windows", "click", "type_text", "press_key", "scroll", "drag", "wait_for_element", "activate_window", "launch_app"]
                },
                "window_title": {"type": "string"},
                "name": {"type": "string"},
                "automation_id": {"type": "string"},
                "control_type": {"type": "string"},
                "class_name": {"type": "string"},
                "text": {"type": "string"},
                "key": {"type": "string", "description": "Key or chord, for example Enter or ctrl+s"},
                "x": {"type": "integer"},
                "y": {"type": "integer"},
                "x2": {"type": "integer"},
                "y2": {"type": "integer"},
                "direction": {"type": "string", "enum": ["up", "down", "left", "right"]},
                "amount": {"type": "integer"},
                "timeout_ms": {"type": "integer", "minimum": 100, "maximum": 120000},
                "app_name": {"type": "string"},
                "include_text": {"type": "boolean"},
                "include_screenshot": {"type": "boolean"},
                "max_elements": {"type": "integer", "minimum": 1, "maximum": 100},
                "grid": {"type": "boolean"},
                "grid_spacing": {"type": "integer", "minimum": 50},
                "output_path": {"type": "string"}
            },
            "required": ["action"]
        })
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            category: "desktop",
            exposure: ToolExposure::Deferred,
            execution: ToolExecutionMode::Exclusive,
            risk: ToolRisk::External,
            timeout_secs: 120,
            produces_artifacts: true,
        }
    }

    fn needs_confirmation(&self, input: &Value) -> bool {
        !matches!(
            input.get("action").and_then(Value::as_str),
            Some("observe" | "list_windows" | "wait_for_element")
        )
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: action"))?;
        let route = match route_action(action, has_selector(&input), cfg!(target_os = "windows")) {
            Ok(route) => route,
            Err(message) => return Ok(ToolResult::err(message)),
        };

        match route {
            BackendRoute::ObserveWindows => observe_windows(&input, ctx).await,
            BackendRoute::Screen(backend_action) => {
                Tool::call(
                    &crate::tools::ScreenTool,
                    screen_input(&input, backend_action),
                    ctx,
                )
                .await
            }
            BackendRoute::Desktop(backend_action) => {
                Tool::call(
                    &crate::tools::DesktopAutomationTool,
                    desktop_input(&input, backend_action),
                    ctx,
                )
                .await
            }
            BackendRoute::Uia(backend_action) => call_uia(&input, backend_action, ctx).await,
        }
    }
}

#[cfg(target_os = "windows")]
async fn call_uia(input: &Value, action: &str, ctx: &ToolContext) -> Result<ToolResult> {
    Tool::call(&crate::tools::UiaTool, uia_input(input, action), ctx).await
}

#[cfg(not(target_os = "windows"))]
async fn call_uia(_input: &Value, _action: &str, _ctx: &ToolContext) -> Result<ToolResult> {
    Ok(ToolResult::err(
        "This semantic desktop action currently requires Windows UI Automation",
    ))
}

#[cfg(target_os = "windows")]
async fn observe_windows(input: &Value, ctx: &ToolContext) -> Result<ToolResult> {
    let include_text = input
        .get("include_text")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_screenshot = input
        .get("include_screenshot")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let force_screen_capture = input.get("output_path").is_some()
        || input.get("grid").and_then(Value::as_bool).unwrap_or(false);

    if include_text {
        let annotated = call_uia(input, "annotate_elements", ctx).await?;
        if !annotated.is_error && include_screenshot && !force_screen_capture {
            return Ok(annotated);
        }
        if !annotated.is_error && !include_screenshot {
            return Ok(ToolResult::ok(annotated.content));
        }
        if include_screenshot {
            let mut captured = Tool::call(
                &crate::tools::ScreenTool,
                screen_input(
                    input,
                    if input.get("window_title").is_some() {
                        "capture_window"
                    } else {
                        "capture"
                    },
                ),
                ctx,
            )
            .await?;
            if !annotated.content.is_empty() {
                captured.content = format!(
                    "{}\n\nAccessibility map: {}",
                    captured.content, annotated.content
                );
            }
            return Ok(captured);
        }
        return Ok(annotated);
    }

    if include_screenshot {
        return Tool::call(
            &crate::tools::ScreenTool,
            screen_input(
                input,
                if input.get("window_title").is_some() {
                    "capture_window"
                } else {
                    "capture"
                },
            ),
            ctx,
        )
        .await;
    }

    Ok(ToolResult::err(
        "observe requires include_text or include_screenshot",
    ))
}

#[cfg(not(target_os = "windows"))]
async fn observe_windows(_input: &Value, _ctx: &ToolContext) -> Result<ToolResult> {
    Ok(ToolResult::err("Windows UI Automation is unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_observe_combines_accessibility_and_screen_backends() {
        assert_eq!(
            route_action("observe", false, true),
            Ok(BackendRoute::ObserveWindows)
        );
    }

    #[test]
    fn selector_actions_prefer_uia_on_windows() {
        assert_eq!(
            route_action("click", true, true),
            Ok(BackendRoute::Uia("click"))
        );
        assert_eq!(
            route_action("type_text", true, true),
            Ok(BackendRoute::Uia("type"))
        );
    }

    #[test]
    fn coordinate_actions_use_desktop_backend() {
        assert_eq!(
            route_action("click", false, true),
            Ok(BackendRoute::Desktop("click"))
        );
        assert_eq!(
            route_action("drag", false, false),
            Ok(BackendRoute::Desktop("drag"))
        );
    }

    #[test]
    fn unsupported_action_is_rejected_before_backend_execution() {
        assert!(route_action("delete_everything", false, true).is_err());
    }
}
