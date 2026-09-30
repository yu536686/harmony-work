use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    /// Supported platforms, e.g. ["windows"], ["linux", "macos"], or empty = all platforms.
    #[serde(default)]
    pub platform: Vec<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub lifecycle: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub skill_id: String,
    pub instructions: String,
    pub source_path: PathBuf,
}

/// Result of a pre-install compatibility check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityCheck {
    pub compatible: bool,
    /// Human-readable reasons why the skill is incompatible (empty = compatible).
    pub issues: Vec<String>,
    /// Warnings that don't block installation but are worth noting.
    pub warnings: Vec<String>,
}

impl CompatibilityCheck {
    #[allow(dead_code)]
    pub fn ok() -> Self {
        Self {
            compatible: true,
            issues: vec![],
            warnings: vec![],
        }
    }
}

/// Check whether a skill can be installed on the current system.
///
/// Checks:
/// 1. `platform` field — must include the current OS (or be empty/absent).
/// 2. `dependencies` field — each entry is checked for availability via PATH lookup.
///    Recognised dependency names: python, python3, node, npm, npx, ruby, go, java, dotnet, git, ffmpeg.
pub async fn check_skill_compatibility(skill: &SkillDefinition) -> CompatibilityCheck {
    let mut issues = Vec::new();
    let mut warnings = Vec::new();

    // ── Platform check ────────────────────────────────────────────────────────
    if !skill.platform.is_empty() {
        let current_os = std::env::consts::OS;
        let supported = skill.platform.iter().any(|platform| {
            let platform = platform.to_lowercase();
            match current_os {
                "windows" => matches!(platform.as_str(), "windows" | "win" | "win32" | "win64"),
                "macos" => matches!(platform.as_str(), "macos" | "mac" | "darwin" | "osx"),
                "linux" => platform == "linux",
                other => platform == other,
            }
        });
        if !supported {
            issues.push(format!(
                "此技能仅支持 {} 平台，当前系统为 {}",
                skill.platform.join(" / "),
                current_os
            ));
        }
    }

    // ── Dependency check ──────────────────────────────────────────────────────
    for dep in &skill.dependencies {
        let dep_lower = dep.to_lowercase();
        // Extract the executable name from entries like "python>=3.8" or "node@18"
        let exe = dep_lower
            .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
            .next()
            .unwrap_or(&dep_lower)
            .to_string();

        // Map common aliases
        let exe = match exe.as_str() {
            "python3" => "python".to_string(),
            "nodejs" => "node".to_string(),
            "dotnet-sdk" | "dotnet-runtime" => "dotnet".to_string(),
            other => other.to_string(),
        };

        // Only check well-known runtimes; ignore vague entries like "office" or "windows"
        let known_runtimes = [
            "python", "node", "npm", "npx", "ruby", "go", "java", "dotnet", "git", "ffmpeg",
            "cargo", "pip", "pip3",
        ];
        if !known_runtimes.contains(&exe.as_str()) {
            continue;
        }

        let found = crate::process::tokio_command("where")
            .arg(&exe)
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !found {
            issues.push(format!("缺少依赖 `{}`：请先安装后再使用此技能", dep));
        }
    }

    // ── Elevated-permission warning ───────────────────────────────────────────
    if !skill.permissions.is_empty() {
        warnings.push(format!(
            "此技能申请了高权限：{}",
            skill.permissions.join(", ")
        ));
    }

    let compatible = issues.is_empty();
    CompatibilityCheck {
        compatible,
        issues,
        warnings,
    }
}

pub struct SkillLoader {
    skills_dir: PathBuf,
    skills: HashMap<String, SkillDefinition>,
}

impl SkillLoader {
    pub fn new(skills_dir: impl Into<PathBuf>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            skills: HashMap::new(),
        }
    }

    pub fn load_all(&mut self) -> Result<()> {
        if !self.skills_dir.exists() {
            std::fs::create_dir_all(&self.skills_dir)?;
            self.create_builtin_skills()?;
        }
        let _ = crate::skills::provenance::ensure_evolution_dirs(&self.skills_dir);
        let _ = crate::skills::provenance::migrate_flat_skills_to_installed(&self.skills_dir);
        self.ensure_harvis_creative_skill()?;

        self.skills.clear();
        let scan_roots = [
            self.skills_dir.clone(),
            crate::skills::provenance::installed_dir(&self.skills_dir),
            crate::skills::provenance::draft_dir(&self.skills_dir),
            crate::skills::provenance::learned_dir(&self.skills_dir),
        ];
        for root in scan_roots {
            if !root.exists() {
                continue;
            }
            let entries = match std::fs::read_dir(&root) {
                Ok(e) => e,
                Err(e) => {
                    warn!("Failed to read skill root {:?}: {}", root, e);
                    continue;
                }
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let skill_file = path.join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                match self.load_skill(&skill_file) {
                    Ok(skill) => {
                        info!("Loaded skill: {} ({})", skill.name, skill.lifecycle);
                        self.skills.insert(skill.skill_id.clone(), skill);
                    }
                    Err(e) => {
                        warn!("Failed to load skill from {:?}: {}", skill_file, e);
                    }
                }
            }
        }
        Ok(())
    }

    fn load_skill(&self, path: &Path) -> Result<SkillDefinition> {
        let raw = std::fs::read(path).with_context(|| format!("Failed to read {:?}", path))?;
        // Strip UTF-8 BOM (EF BB BF) if present so YAML frontmatter parses correctly.
        let raw = if raw.starts_with(b"\xEF\xBB\xBF") {
            &raw[3..]
        } else {
            &raw[..]
        };
        let content = String::from_utf8_lossy(raw).into_owned();

        let (frontmatter, instructions) = parse_frontmatter(&content)?;

        let source = if path.to_string_lossy().contains("\\registry\\")
            || path.to_string_lossy().contains("/registry/")
        {
            "registry".to_string()
        } else if path.to_string_lossy().contains("\\workspace\\")
            || path.to_string_lossy().contains("/workspace/")
        {
            "workspace".to_string()
        } else {
            // Distinguish builtin skills (created by create_builtin_skills) from
            // user-installed skills that also live in the top-level skills dir.
            const BUILTIN_IDS: &[&str] = &[
                "office-automation",
                "file-management",
                "web-automation",
                "system-admin",
                "desktop-control",
                "harvis-creative",
            ];
            let dir_name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if BUILTIN_IDS.contains(&dir_name) {
                "builtin".to_string()
            } else {
                "installed".to_string()
            }
        };

        let tools = frontmatter
            .get("tools")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let unknown_tools: Vec<String> = tools
            .iter()
            .filter(|t| !Self::is_known_tool(t))
            .cloned()
            .collect();
        if !unknown_tools.is_empty() {
            warn!(
                "Skill {:?} declares unknown tools: {:?}",
                path, unknown_tools
            );
        }

        let parse_str_list = |key: &str| -> Vec<String> {
            frontmatter
                .get(key)
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        let skill_id = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();
        let lifecycle =
            crate::skills::provenance::infer_lifecycle_from_path(&self.skills_dir, path);

        Ok(SkillDefinition {
            name: frontmatter
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed")
                .to_string(),
            description: frontmatter
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: frontmatter
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("1.0")
                .to_string(),
            author: frontmatter
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tools,
            dependencies: parse_str_list("dependencies"),
            permissions: parse_str_list("permissions"),
            triggers: parse_str_list("triggers"),
            platform: parse_str_list("platform"),
            source,
            lifecycle: lifecycle.clone(),
            locked: matches!(
                lifecycle.as_str(),
                crate::skills::provenance::LIFECYCLE_BUILTIN
                    | crate::skills::provenance::LIFECYCLE_INSTALLED
            ),
            skill_id,
            instructions,
            source_path: path.parent().unwrap_or(path).to_path_buf(),
        })
    }

    fn create_builtin_skills(&self) -> Result<()> {
        #[allow(clippy::type_complexity)]
        let builtins: Vec<(&str, &str, &str, Vec<&str>, Vec<&str>, &str)> = vec![
            (
                "office-automation",
                "Office Automation",
                "Automate Microsoft Office tasks (Word, Excel, Outlook)",
                vec!["office"],
                vec![
                    "office",
                    "word",
                    "excel",
                    "outlook",
                    "pptx",
                    "PPT",
                    "spreadsheet",
                    "document",
                    "Office自动化",
                    "办公",
                    "表格",
                    "文档",
                    "Word文档",
                    "Excel表格",
                    "幻灯片",
                    "演示文稿",
                    "邮件",
                    "报表",
                    "数据表",
                    "写报告",
                    "制作PPT",
                ],
                "Use the `office` tool to create, edit, and manage Office documents.\n\n\
                 ## Capabilities\n\
                 - Create Word documents with formatted text\n\
                 - Create Excel spreadsheets with data and formulas\n\
                 - Send emails via Outlook\n\
                 - Read and modify existing documents",
            ),
            (
                "file-management",
                "File Management",
                "Organize, search, and manage files on the system",
                vec!["file_read", "file_write", "shell"],
                vec![
                    "file",
                    "files",
                    "folder",
                    "directory",
                    "rename",
                    "move",
                    "copy",
                    "delete",
                    "文件",
                    "文件夹",
                    "目录",
                    "整理",
                    "搜索文件",
                    "批量重命名",
                    "移动文件",
                    "复制文件",
                    "删除文件",
                    "文件管理",
                    "查找文件",
                ],
                "Use file tools to manage the user's files.\n\n\
                 ## Capabilities\n\
                 - Read and write text files\n\
                 - Search for files using shell commands\n\
                 - Organize files into directories\n\
                 - Batch rename and move files",
            ),
            (
                "web-automation",
                "Web Automation",
                "Automate web browsing tasks using Edge or Chrome",
                vec!["browser", "web_search", "web_fetch"],
                vec![
                    "browser",
                    "web",
                    "chrome",
                    "crawl",
                    "scrape",
                    "search",
                    "navigate",
                    "url",
                    "网页",
                    "浏览器",
                    "爬虫",
                    "抓取",
                    "网络",
                    "搜索",
                    "自动填表",
                    "网页自动化",
                    "打开网页",
                    "浏览网页",
                    "数据抓取",
                ],
                "Use the Playwright MCP browser tools to automate web tasks. Start with browser_navigate and browser_snapshot; use accessible element references for actions.\n\n\
                 ## Capabilities\n\
                 - Navigate to URLs\n\
                 - Fill forms and click buttons\n\
                 - Extract data from web pages\n\
                 - Take screenshots of web content",
            ),
            (
                "system-admin",
                "System Administration",
                "Manage Windows system settings and processes",
                vec!["system_info", "powershell_query", "shell"],
                vec![
                    "system",
                    "windows",
                    "powershell",
                    "process",
                    "service",
                    "registry",
                    "admin",
                    "系统",
                    "系统管理",
                    "进程",
                    "服务",
                    "注册表",
                    "系统设置",
                    "系统信息",
                    "系统监控",
                    "系统维护",
                    "Windows管理",
                    "系统优化",
                ],
                "Use `system_info` and `powershell_query` to manage the Windows system.\n\n\
                 ## Capabilities\n\
                 - Query system information\n\
                 - Manage services and processes\n\
                 - Configure system settings\n\
                 - Monitor system health",
            ),
            (
                "desktop-control",
                "Desktop Control",
                "Control local desktop applications through unified computer use",
                vec!["computer_use"],
                vec![
                    "desktop",
                    "uia",
                    "automation",
                    "click",
                    "type",
                    "screenshot",
                    "window",
                    "桌面",
                    "桌面控制",
                    "UI自动化",
                    "点击",
                    "输入",
                    "截图",
                    "窗口",
                    "自动化操作",
                    "界面操作",
                    "桌面自动化",
                    "应用控制",
                ],
                "Use the windows_desktop MCP tools to observe and control desktop applications. Prefer UIA3 snapshots and element references; use screenshots when accessibility data is insufficient.\n\n\
                 ## Capabilities\n\
                 - Find and interact with UI elements\n\
                 - Click buttons, type text, send hotkeys\n\
                 - Take screenshots for visual verification\n\
                 - Automate multi-step desktop workflows",
            ),
        ];

        for (id, name, desc, tools, triggers, instructions) in builtins {
            let skill_dir = self.skills_dir.join(id);
            std::fs::create_dir_all(&skill_dir)?;
            let tools_yaml: Vec<String> = tools.iter().map(|t| format!("  - {}", t)).collect();
            let triggers_yaml: Vec<String> =
                triggers.iter().map(|t| format!("  - \"{}\"", t)).collect();
            let content = format!(
                "---\nname: {}\ndescription: {}\nversion: \"1.0\"\ntools:\n{}\ntriggers:\n{}\n---\n\n# {}\n\n{}\n",
                name,
                desc,
                tools_yaml.join("\n"),
                triggers_yaml.join("\n"),
                name,
                instructions
            );
            std::fs::write(skill_dir.join("SKILL.md"), content)?;
        }
        Ok(())
    }

    /// Ship Harvis creation as a locked system skill for both clean installs and
    /// upgrades where the skills root already exists. Existing user-authored
    /// skills are never touched.
    fn ensure_harvis_creative_skill(&self) -> Result<()> {
        let skill_dir = self.skills_dir.join("harvis-creative");
        let skill_file = skill_dir.join("SKILL.md");
        const HARVIS_SKILL_VERSION: &str = "2.3";
        if let Ok(existing) = std::fs::read_to_string(&skill_file) {
            if existing.contains(&format!("version: \"{}\"", HARVIS_SKILL_VERSION)) {
                return Ok(());
            }
        }
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(
            skill_file,
            r#"---
name: harvis-creative
description: Use when the user asks to generate background music, instrumental music, images, posters, campaign visuals, or image variations through Harvis.
version: "2.3"
tools:
  - harvis_image_generate
  - harvis_music_generate
  - harvis_job_status
  - harvis_save_result
triggers:
  - "生成音乐"
  - "背景音乐"
  - "BGM"
  - "生成图片"
  - "海报"
  - "文生图"
  - "图生图"
---

# Harvis AI 创作

仅在用户明确要求生成音乐或图片时使用本技能。先组织完整请求，不要为了规划反复调用工具。若工具尚未出现在当前工具列表，先调用一次 `tool_search` 搜索 `Harvis image poster` 或 `Harvis music BGM`，再直接调用匹配工具。

## 音乐

调用 `harvis_music_generate`，最小请求为：

```json
{"prompt":"elegant luxury instrumental, piano and strings, 45-second brand BGM, no vocals","title":"品牌 BGM","duration":45}
```

默认使用纯音乐。给出简洁英文 `prompt`、标题和时长；`tags` 是可选的精简风格词，控制在 900 字符以内。完整创作要求写入 `prompt`，不要将长篇方案直接写进 `tags`。除非用户明确要求人声，否则加入 `instrumental only, no vocals`，不要写歌词或演唱要求。

## 图片

文生图直接调用 `harvis_image_generate`，不需要参考图，也不要声称 Harvis 只能图生图：

```json
{"prompt":"高端腕表主视觉海报，香槟金棚拍，留白构图","canvas_preset":"vertical"}
```

比例映射：电视屏幕 `television`、竖屏 `vertical`、门店海报 `store-poster`、手机全屏 `phone-fullscreen`、自定义 `custom`（同时提供 `output_width` 和 `output_height`）。只有用户提供可公开访问的参考图 URL 时才传 `reference_urls`；旧素材 ID 不能直接作为参考图地址。不传即为文生图。

## 进度与交付

Harvis 工具使用客户端已配置的个人 Harmony API Key 和对应创作模型。生成工具提交后立即返回 `job_id`；用同一 `job_id` 调用 `harvis_job_status` 查询进度，每次查询最多等待 20 秒。最多继续查询 3 次；仍未完成时向用户说明任务仍在生成并保留 `job_id`，不要无限轮询或重新提交以免重复计费。

任务成功后在回复中嵌入图片成品的 Markdown 图片链接（音乐则给播放/下载链接），并调用 `harvis_save_result` 选择 `workspace`，将成品存到当前会话工作区的 `marketing-assets` 目录，给出本地路径。新的 Harmony Responses 任务不能直接用 `material_library` 归档；如需进入素材库，先保存到工作区再由用户导入。
"#,
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_skill(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.get(name)
    }

    pub fn list_skills(&self) -> Vec<&SkillDefinition> {
        self.skills.values().collect()
    }

    /// Parse a SkillDefinition from raw SKILL.md content without writing to disk.
    /// Used by install_skill to validate before writing.
    pub fn parse_skill_from_content(&self, content: &str) -> Result<SkillDefinition> {
        let tmp = std::path::Path::new("skill.md");
        let (frontmatter, instructions) = parse_frontmatter(content)?;

        let tools = frontmatter
            .get("tools")
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let parse_str_list = |key: &str| -> Vec<String> {
            frontmatter
                .get(key)
                .and_then(|v| v.as_sequence())
                .map(|seq| {
                    seq.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok(SkillDefinition {
            name: frontmatter
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed")
                .to_string(),
            description: frontmatter
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            version: frontmatter
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("1.0")
                .to_string(),
            author: frontmatter
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tools,
            dependencies: parse_str_list("dependencies"),
            permissions: parse_str_list("permissions"),
            triggers: parse_str_list("triggers"),
            platform: parse_str_list("platform"),
            source: "installed".to_string(),
            lifecycle: crate::skills::provenance::LIFECYCLE_INSTALLED.to_string(),
            locked: true,
            skill_id: frontmatter
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unnamed")
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .to_lowercase(),
            instructions,
            source_path: tmp.to_path_buf(),
        })
    }

    #[allow(dead_code)]
    pub fn generate_skill_prompt(&self, enabled_skills: &[String]) -> String {
        let mut prompt = String::new();
        for name in enabled_skills {
            if let Some(skill) = self.skills.get(name) {
                prompt.push_str(&format!(
                    "\n## Skill: {}\nSource: {}\nPermissions: {}\n{}\n\n{}\n",
                    skill.name,
                    skill.source,
                    if skill.permissions.is_empty() {
                        "none".to_string()
                    } else {
                        skill.permissions.join(", ")
                    },
                    skill.description,
                    skill.instructions
                ));
            }
        }
        prompt
    }

    /// Get the full instructions for a skill by name.
    #[allow(dead_code)]
    pub fn get_skill_instructions(&self, name: &str) -> Option<String> {
        self.skills.get(name).map(|s| {
            format!(
                "## Skill: {}\n{}\n\nPermissions: {}\n\n{}",
                s.name,
                s.description,
                if s.permissions.is_empty() {
                    "none".to_string()
                } else {
                    s.permissions.join(", ")
                },
                s.instructions
            )
        })
    }

    fn is_known_tool(name: &str) -> bool {
        matches!(
            name,
            "file_read"
                | "file_write"
                | "file_edit"
                | "file_diff"
                | "file_search"
                | "file_list"
                | "code_run"
                | "process_control"
                | "shell"
                | "web_search"
                | "web_fetch"
                | "powershell_query"
                | "memory_store"
                | "plan_todo"
                | "vision_context"
                | "call_fish"
                | "call_koi"
                | "pool_org"
                | "pool_chat"
                | "app_control"
                | "im_send_message"
                | "skill_list"
                | "skill_search"
                | "ssh"
                | "pdf"
                | "wmi"
                | "office"
                | "browser"
                | "computer_use"
                | "uia"
                | "screen_capture"
                | "com"
                | "com_invoke"
                | "email"
        )
    }
}

fn parse_frontmatter(content: &str) -> Result<(serde_yaml::Value, String)> {
    let content = content.trim();
    if let Some(stripped) = content.strip_prefix("---") {
        if let Some(end) = stripped.find("---") {
            let yaml_str = &stripped[..end];
            let instructions = stripped[end + 3..].trim().to_string();
            let frontmatter: serde_yaml::Value = serde_yaml::from_str(yaml_str)
                .with_context(|| "Failed to parse YAML frontmatter")?;
            return Ok((frontmatter, instructions));
        }
    }
    Ok((
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        content.to_string(),
    ))
}
