pub const DISPLAY_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PINNED_CODEX_VERSION: &str = "0.156.1";
pub const MANAGED_MCP_BEGIN: &str = "# BEGIN WORKBUDDY MCP";
pub const MANAGED_MCP_END: &str = "# END WORKBUDDY MCP";
const WORKBUDDY_MODEL_CATALOG: &[u8] = include_bytes!("../../resources/workbuddy-models.json");

use crate::platform_profile::DesktopPlatform;
use std::path::{Path, PathBuf};

/// Give Codex auto-review a model actually served by the Harmony gateway.
/// A user's explicit model catalog always takes precedence.
pub fn ensure_managed_auto_review_catalog(
    codex_home: &Path,
    existing_config: &str,
) -> std::io::Result<String> {
    let has_custom_catalog = existing_config
        .lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .any(|line| line.trim_start().starts_with("model_catalog_json"));
    if has_custom_catalog {
        return Ok(existing_config.to_string());
    }

    let catalog_path = codex_home.join("workbuddy-models.json");
    let catalog_is_current = std::fs::read(&catalog_path)
        .map(|existing| existing == WORKBUDDY_MODEL_CATALOG)
        .unwrap_or(false);
    if !catalog_is_current {
        std::fs::write(&catalog_path, WORKBUDDY_MODEL_CATALOG)?;
    }
    let catalog_path = catalog_path.to_string_lossy().replace('\\', "/");
    let value = toml::Value::String(catalog_path);
    Ok(format!("model_catalog_json = {value}\n{existing_config}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexRuntimeConfig {
    pub package_version: &'static str,
    pub display_version: &'static str,
    pub codex_version: &'static str,
}

impl CodexRuntimeConfig {
    pub const fn b1_default() -> Self {
        Self {
            package_version: env!("CARGO_PKG_VERSION"),
            display_version: DISPLAY_VERSION,
            codex_version: PINNED_CODEX_VERSION,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeepSeekProfile {
    pub provider_id: &'static str,
    pub model: &'static str,
    pub base_url: &'static str,
    pub env_key: &'static str,
}

impl Default for DeepSeekProfile {
    fn default() -> Self {
        Self {
            provider_id: "deepseek",
            model: "deepseek-v4-pro",
            base_url: "https://api.deepseek.com",
            env_key: "DEEPSEEK_API_KEY",
        }
    }
}

impl DeepSeekProfile {
    pub fn render_toml(self) -> String {
        format!(
            "model = \"{}\"\nmodel_provider = \"{}\"\n\n[model_providers.{}]\nname = \"DeepSeek\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nsupports_websockets = false\n",
            self.model, self.provider_id, self.provider_id, self.base_url, self.env_key
        )
    }
}

/// Harmony 内部网关供应商：走 OpenAI Responses 协议，模型与额度由网关统一管理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarmonyProfile {
    pub provider_id: &'static str,
    pub model: &'static str,
    pub base_url: &'static str,
    pub env_key: &'static str,
    pub display_name: &'static str,
}

impl Default for HarmonyProfile {
    fn default() -> Self {
        Self {
            provider_id: "harmony",
            model: "deepseek-v4-flash",
            base_url: crate::service_urls::HARMONY_API_BASE,
            env_key: "HARMONY_API_KEY",
            display_name: "Harmony",
        }
    }
}

impl HarmonyProfile {
    /// 用当前 model / base_url / env_key 渲染 codex.toml 的 LLM 顶部配置段。
    /// wire_api = "responses" 保证 Codex Responses 协议（与 DeepSeek 一致）。
    pub fn render_toml(self) -> String {
        format!(
            "model = \"{}\"\nmodel_provider = \"{}\"\n\n[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nsupports_websockets = false\n",
            self.model, self.provider_id, self.provider_id, self.display_name, self.base_url, self.env_key
        )
    }
}

/// 渲染一份同时含 DeepSeek（默认，保持原行为）与 Harmony（内部网关备选）的 codex.toml 初始配置。
/// DeepSeek 作为默认 model_provider 不变；Harmony 作为额外 provider 段，用户可在设置中切换。
pub fn render_default_providers_toml() -> String {
    let deepseek = DeepSeekProfile::default();
    let harmony = HarmonyProfile::default();
    format!(
        "{}{}",
        deepseek.render_toml(),
        format!(
            "\n[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nsupports_websockets = false\n",
            harmony.provider_id, harmony.display_name, harmony.base_url, harmony.env_key
        )
    )
}

/// 渲染 Harmony 内部网关的 provider 段（不含顶层 model / model_provider），
/// 用于在升级场景（已有 codex.toml 但缺少 harmony 段）时补齐配置。
pub fn render_harmony_provider_section() -> String {
    let harmony = HarmonyProfile::default();
    format!(
        "\n[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nsupports_websockets = false\n",
        harmony.provider_id, harmony.display_name, harmony.base_url, harmony.env_key
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBuddyMcpProfile {
    executable: PathBuf,
    app_data_dir: PathBuf,
    skills_roots: Vec<PathBuf>,
    marketing_base_url: String,
}

impl WorkBuddyMcpProfile {
    pub fn new(
        executable: &Path,
        app_data_dir: &Path,
        skills_roots: &[&Path],
        marketing_base_url: &str,
    ) -> Self {
        Self {
            executable: executable.to_path_buf(),
            app_data_dir: app_data_dir.to_path_buf(),
            skills_roots: skills_roots.iter().map(|path| path.to_path_buf()).collect(),
            marketing_base_url: marketing_base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn render_toml(&self) -> String {
        let database_path = self.app_data_dir.join("workbuddy.db");
        let skills = std::env::join_paths(&self.skills_roots)
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|_| {
                self.skills_roots
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(if cfg!(windows) { ";" } else { ":" })
            });
        format!(
            "[mcp_servers.workbuddy]\ncommand = {}\nstartup_timeout_sec = 15\ntool_timeout_sec = 180\nenv = {{ WORKBUDDY_APP_DATA_DIR = {}, WORKBUDDY_DATABASE_PATH = {}, WORKBUDDY_SKILLS_DIRS = {}, WORKBUDDY_MARKETING_URL = {}, WORKBUDDY_MARKETING_SESSION_DIR = {} }}\n",
            toml_string(&self.executable.display().to_string()),
            toml_string(&self.app_data_dir.display().to_string()),
            toml_string(&database_path.display().to_string()),
            toml_string(&skills),
            toml_string(&self.marketing_base_url),
            toml_string(&crate::app::logging::default_app_data_dir().display().to_string()),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationMcpProfile {
    node: PathBuf,
    playwright_entry: PathBuf,
    windows_desktop: Option<PathBuf>,
    macos_desktop: Option<PathBuf>,
    headless: bool,
    browser_channel: String,
}

impl AutomationMcpProfile {
    pub fn new(
        node: &Path,
        playwright_entry: &Path,
        windows_desktop: Option<&Path>,
        headless: bool,
    ) -> Self {
        Self {
            node: node.to_path_buf(),
            playwright_entry: playwright_entry.to_path_buf(),
            windows_desktop: windows_desktop.map(Path::to_path_buf),
            macos_desktop: None,
            headless,
            browser_channel: DesktopPlatform::current().browser_channel().to_string(),
        }
    }

    pub fn with_browser_channel(mut self, channel: &str) -> Self {
        self.browser_channel = channel.to_string();
        self
    }

    pub fn with_macos_desktop(mut self, entry: Option<&Path>) -> Self {
        self.macos_desktop = entry.map(Path::to_path_buf);
        self
    }

    pub fn render_toml(&self) -> String {
        let mut rendered = String::new();
        if !self.node.as_os_str().is_empty() && !self.playwright_entry.as_os_str().is_empty() {
            let mut args = vec![
                toml_string(&self.playwright_entry.display().to_string()),
                toml_string(&format!("--browser={}", self.browser_channel)),
            ];
            if self.headless {
                args.push(toml_string("--headless"));
            }
            rendered.push_str(&format!(
                "[mcp_servers.playwright]\ncommand = {}\nargs = [{}]\nstartup_timeout_sec = 30\ntool_timeout_sec = 180\n",
                toml_string(&self.node.display().to_string()),
                args.join(", ")
            ));
        }
        if let Some(executable) = self
            .windows_desktop
            .as_ref()
            .filter(|path| !path.as_os_str().is_empty())
        {
            rendered.push_str(&format!(
                "\n[mcp_servers.windows_desktop]\ncommand = {}\nstartup_timeout_sec = 30\ntool_timeout_sec = 180\n",
                toml_string(&executable.display().to_string())
            ));
        }
        if let Some(entry) = self.macos_desktop.as_ref().filter(|_| !self.node.as_os_str().is_empty()) {
            rendered.push_str(&format!(
                "\n[mcp_servers.macos_desktop]\ncommand = {}\nargs = [{}]\nstartup_timeout_sec = 30\ntool_timeout_sec = 180\n",
                toml_string(&self.node.display().to_string()),
                toml_string(&entry.display().to_string()),
            ));
        }
        rendered
    }
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

pub fn migrate_public_service_config(existing: &str) -> String {
    let Ok(mut doc) = existing.parse::<toml_edit::Document>() else {
        return existing.to_string();
    };
    let mut changed = false;
    for (group, field) in [("model_providers", "base_url"), ("mcp_servers", "url")] {
        if let Some(entries) = doc.get_mut(group).and_then(|item| item.as_table_like_mut()) {
            for (_, entry) in entries.iter_mut() {
                if let Some(base) = entry.get_mut(field) {
                    if let Some(raw) = base.as_str() {
                        let migrated = crate::service_urls::migrate_public_service_url(raw);
                        if migrated != raw {
                            let decor = base.as_value().map(|value| value.decor().clone());
                            let mut replacement = toml_edit::Value::from(migrated);
                            if let Some(decor) = decor {
                                *replacement.decor_mut() = decor;
                            }
                            *base = toml_edit::Item::Value(replacement);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    if changed {
        doc.to_string()
    } else {
        existing.to_string()
    }
}

pub fn merge_managed_mcp_config(existing: &str, managed: &str) -> String {
    let block = format!(
        "{MANAGED_MCP_BEGIN}\n{}{MANAGED_MCP_END}",
        managed.trim_end().to_string() + "\n"
    );
    if let Some(begin) = existing.find(MANAGED_MCP_BEGIN) {
        if let Some(relative_end) = existing[begin..].find(MANAGED_MCP_END) {
            let end = begin + relative_end + MANAGED_MCP_END.len();
            return format!("{}{}{}", &existing[..begin], block, &existing[end..]);
        }
    }
    let separator = if existing.is_empty() || existing.ends_with("\n\n") {
        ""
    } else if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{existing}{separator}{block}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn managed_auto_review_catalog_uses_a_real_gateway_model_and_is_idempotent() {
        let codex_home = std::env::temp_dir().join(format!(
            "harmony-review-catalog-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&codex_home).unwrap();
        let original = "model = \"deepseek-v4-flash\"\n[model_providers.harmony]\nbase_url = \"https://api.harmonywatch.cn/v1\"\n";

        let configured = ensure_managed_auto_review_catalog(&codex_home, original).unwrap();
        let parsed: toml::Value = configured.parse().unwrap();
        let catalog_path = parsed["model_catalog_json"].as_str().unwrap();
        assert!(catalog_path.ends_with("/workbuddy-models.json"));
        assert_eq!(
            parsed["model_providers"]["harmony"]["base_url"].as_str(),
            Some("https://api.harmonywatch.cn/v1")
        );
        let catalog: serde_json::Value = serde_json::from_slice(
            &std::fs::read(codex_home.join("workbuddy-models.json")).unwrap(),
        )
        .unwrap();
        let flash = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .find(|model| model["slug"] == "deepseek-v4-flash")
            .unwrap();
        assert_eq!(flash["auto_review_model_override"], "deepseek-v4-pro");
        assert_eq!(
            ensure_managed_auto_review_catalog(&codex_home, &configured).unwrap(),
            configured
        );

        std::fs::remove_dir_all(codex_home).unwrap();
    }

    #[test]
    fn managed_auto_review_catalog_respects_an_existing_custom_catalog() {
        let codex_home = std::env::temp_dir().join(format!(
            "harmony-custom-catalog-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&codex_home).unwrap();
        let custom = "model_catalog_json = \"C:/custom/models.json\"\n[model_providers.harmony]\nbase_url = \"https://api.harmonywatch.cn/v1\"\n";

        assert_eq!(
            ensure_managed_auto_review_catalog(&codex_home, custom).unwrap(),
            custom
        );
        assert!(!codex_home.join("workbuddy-models.json").exists());

        std::fs::remove_dir_all(codex_home).unwrap();
    }

    #[test]
    fn public_service_migration_preserves_user_config_and_managed_markers() {
        let old = r#"# User configuration
model = "my-model"
[model_providers.harmony]
base_url = "https://api.harmonywatch.cn:13001/v1" # account gateway
env_key = "HARMONY_API_KEY"
[model_providers.local]
base_url = "http://192.168.11.29:13000/v1"
[model_providers.custom]
base_url = "https://custom.example:13001/v1"
[mcp_servers.remote]
url = "http://www.harmonywatch.cn:8090/mcp"
bearer_token_env_var = "MCP_ACCOUNT_TOKEN"
[mcp_servers.local]
url = "http://192.168.11.29:1933/mcp"
# BEGIN WORKBUDDY MCP
[mcp_servers.workbuddy]
command = "host"
# END WORKBUDDY MCP
"#;
        let migrated = migrate_public_service_config(old);
        let parsed: toml::Value = migrated.parse().unwrap();
        assert_eq!(
            parsed["model_providers"]["harmony"]["base_url"].as_str(),
            Some(crate::service_urls::HARMONY_API_BASE)
        );
        assert!(migrated.contains("# User configuration"));
        assert!(migrated.contains("# account gateway"));
        assert!(migrated.contains(MANAGED_MCP_BEGIN));
        assert!(migrated.contains(MANAGED_MCP_END));
        assert!(migrated.contains("HARMONY_API_KEY"));
        assert_eq!(
            parsed["mcp_servers"]["remote"]["url"].as_str(),
            Some("https://portal.harmonywatch.cn/mcp")
        );
        assert!(migrated.contains("MCP_ACCOUNT_TOKEN"));
        assert!(migrated.contains("http://192.168.11.29:1933/mcp"));
        assert!(migrated.contains("http://192.168.11.29:13000/v1"));
        assert!(migrated.contains("https://custom.example:13001/v1"));
        assert_eq!(migrate_public_service_config(&migrated), migrated);
        assert_eq!(migrate_public_service_config("invalid = ["), "invalid = [");
    }

    #[test]
    fn b1_version_has_semver_and_display_forms() {
        let config = CodexRuntimeConfig::b1_default();

        assert_eq!(config.package_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(config.display_version, config.package_version);
        assert_eq!(config.codex_version, "0.156.1");
    }

    #[test]
    fn deepseek_profile_uses_responses_without_persisting_secret() {
        let profile = DeepSeekProfile::default();
        let rendered = profile.render_toml();

        assert!(rendered.contains("base_url = \"https://api.deepseek.com\""));
        assert!(rendered.contains("env_key = \"DEEPSEEK_API_KEY\""));
        assert!(rendered.contains("wire_api = \"responses\""));
        assert!(!rendered.contains("sk-"));
        assert!(!rendered.contains("api_key"));
    }

    #[test]
    fn managed_mcp_profile_registers_native_host_and_domain_paths() {
        let rendered = WorkBuddyMcpProfile::new(
            Path::new("C:/Program Files/Xiaoheng/workbuddy-mcp-host.exe"),
            Path::new("C:/Users/test/AppData/Roaming/WorkBuddy"),
            &[Path::new("C:/Users/test/AppData/Roaming/WorkBuddy/skills")],
            "https://www.harmonywatch.cn:8090",
        )
        .render_toml();

        assert!(rendered.contains("[mcp_servers.workbuddy]"));
        assert!(rendered.contains("workbuddy-mcp-host.exe"));
        assert!(rendered.contains("WORKBUDDY_DATABASE_PATH"));
        assert!(rendered.contains("workbuddy.db"));
        assert!(rendered.contains("WORKBUDDY_MARKETING_URL"));
        assert!(rendered.contains("https://www.harmonywatch.cn:8090"));
        assert!(!rendered.contains("piscis"));
    }

    #[test]
    fn replaces_managed_mcp_block_without_overwriting_user_config() {
        let old = "model = \"deepseek-v4-pro\"\n# BEGIN WORKBUDDY MCP\nold\n# END WORKBUDDY MCP\n[custom]\nkeep = true\n";
        let merged = merge_managed_mcp_config(old, "[mcp_servers.workbuddy]\ncommand = \"host\"\n");
        assert!(merged.contains("model = \"deepseek-v4-pro\""));
        assert!(merged.contains("command = \"host\""));
        assert!(merged.contains("[custom]\nkeep = true"));
        assert!(!merged.contains("\nold\n"));
    }

    #[test]
    fn automation_mcp_profile_uses_bundled_executables_and_headless_setting() {
        let profile = AutomationMcpProfile::new(
            Path::new("C:/Harmony/runtime/node/node.exe"),
            Path::new("C:/Program Files/Harmony Work/resources/playwright-mcp/cli.js"),
            Some(Path::new(
                "C:/Program Files/Harmony Work/resources/flaui-mcp/FlaUI.Mcp.exe",
            )),
            true,
        );
        let rendered = profile.render_toml();
        let parsed: toml::Value = rendered.parse().expect("valid managed MCP TOML");

        assert_eq!(
            parsed["mcp_servers"]["playwright"]["command"].as_str(),
            Some("C:/Harmony/runtime/node/node.exe")
        );
        assert_eq!(
            parsed["mcp_servers"]["playwright"]["args"][0].as_str(),
            Some("C:/Program Files/Harmony Work/resources/playwright-mcp/cli.js")
        );
        let expected_browser = if cfg!(windows) {
            "\"--browser=msedge\""
        } else {
            "\"--browser=chrome\""
        };
        assert!(rendered.contains(expected_browser));
        assert!(rendered.contains("\"--headless\""));
        assert!(rendered.contains("[mcp_servers.windows_desktop]"));
        assert!(!rendered.contains("npx"));
        assert!(!rendered.contains("robotz"));
    }

    #[test]
    fn automation_mcp_profile_skips_unavailable_servers() {
        let profile = AutomationMcpProfile::new(
            Path::new(""),
            Path::new(""),
            None,
            false,
        );
        assert!(profile.render_toml().is_empty());
    }

    #[test]
    fn browser_channel_does_not_depend_on_desktop_server() {
        let profile = AutomationMcpProfile::new(
            Path::new("C:/Node/node.exe"),
            Path::new("C:/App/playwright-mcp/cli.js"),
            None,
            false,
        );
        let rendered = profile.render_toml();
        let expected = if cfg!(windows) {
            "--browser=msedge"
        } else {
            "--browser=chrome"
        };
        assert!(rendered.contains(expected));
        assert!(!rendered.contains("--headless"));
        assert!(!rendered.contains("windows_desktop"));
    }

    #[test]
    fn browser_channel_can_follow_installed_chrome() {
        let profile = AutomationMcpProfile::new(
            Path::new("C:/Node/node.exe"),
            Path::new("C:/App/playwright-mcp/cli.js"),
            None,
            true,
        )
        .with_browser_channel("chrome");
        let rendered = profile.render_toml();
        assert!(rendered.contains("--browser=chrome"));
        assert!(!rendered.contains("--browser=msedge"));
    }

    #[test]
    fn macos_desktop_uses_bundled_node_without_changing_windows_profile() {
        let profile = AutomationMcpProfile::new(
            Path::new("/Applications/Harmony Work.app/Contents/Resources/resources/node/bin/node"),
            Path::new(""),
            None,
            false,
        )
        .with_macos_desktop(Some(Path::new(
            "/Applications/Harmony Work.app/Contents/Resources/resources/mac-use-mcp/dist/helpers/cli.mjs",
        )));
        let rendered = profile.render_toml();
        let parsed: toml::Value = rendered.parse().expect("valid managed MCP TOML");
        assert_eq!(
            parsed["mcp_servers"]["macos_desktop"]["args"][0].as_str(),
            Some("/Applications/Harmony Work.app/Contents/Resources/resources/mac-use-mcp/dist/helpers/cli.mjs")
        );
        assert!(!rendered.contains("windows_desktop"));
    }
}
