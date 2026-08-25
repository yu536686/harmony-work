use crate::store::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinToolInfo {
    pub name: String,
    pub description: String,
    pub icon: String,
    pub windows_only: bool,
}

/// Returns the list of system built-in tools with metadata.
#[tauri::command]
pub async fn list_builtin_tools(
    _state: State<'_, AppState>,
) -> Result<Vec<BuiltinToolInfo>, String> {
    Ok(builtin_tool_catalog())
}

fn builtin_tool_catalog() -> Vec<BuiltinToolInfo> {
    vec![
        BuiltinToolInfo {
            name: "file_read".into(),
            description: "读取本地文件内容，支持文本和二进制文件".into(),
            icon: "📄".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "file_write".into(),
            description: "写入或修改本地文件，支持创建新文件和追加内容".into(),
            icon: "✏️".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "shell".into(),
            description: "执行系统 Shell 命令（cmd.exe / bash）".into(),
            icon: "⌨️".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "powershell_query".into(),
            description: "执行 PowerShell 脚本，支持 Windows 系统管理任务".into(),
            icon: "🪟".into(),
            windows_only: true,
        },
        BuiltinToolInfo {
            name: "web_search".into(),
            description: "搜索互联网，获取最新信息".into(),
            icon: "🔍".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "web_fetch".into(),
            description: "抓取指定 URL 的正文（HTML 转纯文本），适合读文档与 changelogs".into(),
            icon: "📰".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "browser".into(),
            description: "控制 Chrome 浏览器，支持网页导航、点击、截图等操作".into(),
            icon: "🌐".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "document_artifact".into(),
            description: "通过结构化内容创建并验证 DOCX 文档，不依赖 Microsoft Word".into(),
            icon: "DOCX".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "presentation_artifact".into(),
            description: "通过固定版式创建并验证 PPTX 演示文稿，不依赖 PowerPoint COM".into(),
            icon: "PPTX".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "spreadsheet_artifact".into(),
            description: "通过结构化工作表创建并验证 XLSX 文件，支持公式、筛选和冻结表头".into(),
            icon: "XLSX".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "artifact_render".into(),
            description: "使用 LibreOffice 将 Office 成果渲染为 PDF 或页面图片以便视觉检查".into(),
            icon: "PDF".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "email".into(),
            description: "通过 SMTP 发送邮件（需在设置中配置）".into(),
            icon: "📧".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "plan_todo".into(),
            description: "维护当前复杂任务的可视化执行计划与待办状态".into(),
            icon: "📋".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "vision_context".into(),
            description: "管理可复用的视觉工件，并决定下一轮要送入多模态模型的图片".into(),
            icon: "🖼️".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "harmony_cut".into(),
            description: "Harmony Cut commercial editing: analyze local video materials, create a review workbench, and continue to preview plus CapCut/JianYing draft generation.".into(),
            icon: "CUT".into(),
            windows_only: true,
        },
        BuiltinToolInfo {
            name: "harvis_creative".into(),
            description: "通过已登录的 Harmony 网关创建 Harvis 音乐和图片、查询创作进度，并将完成的成品归档至素材库".into(),
            icon: "AI".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "computer_use".into(),
            description: "统一感知和操作本地桌面应用，自动选择 UI Automation、截图和键鼠后端".into(),
            icon: "🖱️".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "pool_org".into(),
            description: "创建和管理项目池与组织规范，由小亨协调 StaffDeck 人员执行".into(),
            icon: "🏊".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "im_channel_list".into(),
            description: "查看当前已注册 IM 渠道的连接状态与可用性".into(),
            icon: "📡".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "im_channel_connect".into(),
            description: "连接设置中已启用的 IM 渠道，不提供断开能力".into(),
            icon: "🔌".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "im_channel_binding_lookup".into(),
            description: "按 session_id、pool_id 或 task_id 查询可用的 IM binding_key".into(),
            icon: "🧭".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "im_channel_binding_list".into(),
            description: "按 channel 名列出可用的 IM token 候选与 binding_key".into(),
            icon: "🎯".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "skill_list".into(),
            description: "列出已加载技能及触发条件，支持渐进式披露深读 SKILL.md".into(),
            icon: "⚡".into(),
            windows_only: false,
        },
        BuiltinToolInfo {
            name: "skill_manage".into(),
            description: "创建/修改 Agent 草稿技能（installed/locked 技能受保护）".into(),
            icon: "🧬".into(),
            windows_only: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_exposes_unified_computer_use_only() {
        let names = builtin_tool_catalog()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name == "computer_use"));
        for artifact_tool in [
            "document_artifact",
            "presentation_artifact",
            "spreadsheet_artifact",
            "artifact_render",
        ] {
            assert!(
                names.iter().any(|name| name == artifact_tool),
                "{artifact_tool} must appear in the user-facing tool catalog"
            );
        }
        for hidden in [
            "call_fish",
            "call_koi",
            "uia",
            "screen_capture",
            "desktop_automation",
            "com",
            "office",
            "presentation",
            "com_invoke",
            "wmi",
        ] {
            assert!(
                !names.iter().any(|name| name == hidden),
                "{hidden} must not appear in the user-facing tool catalog"
            );
        }
    }
}

/// Manually trigger a heartbeat agent run.
#[tauri::command]
pub async fn trigger_heartbeat(state: State<'_, AppState>) -> Result<(), String> {
    let (prompt, enabled) = {
        let settings = state.settings.lock().await;
        (
            settings.heartbeat_prompt.clone(),
            settings.heartbeat_enabled,
        )
    };
    if !enabled {
        return Err("Heartbeat is not enabled in settings".into());
    }
    let state_ref = (*state).clone();
    tokio::spawn(async move {
        let _ = crate::coordination::heartbeat::dispatch_heartbeat(&state_ref, &prompt, "internal")
            .await;
    });
    Ok(())
}
