pub const DISPLAY_VERSION: &str = "b1.0.5";
pub const PINNED_CODEX_VERSION: &str = "0.149.0";
pub const MANAGED_MCP_BEGIN: &str = "# BEGIN WORKBUDDY MCP";
pub const MANAGED_MCP_END: &str = "# END WORKBUDDY MCP";

use std::path::{Path, PathBuf};

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
            "[mcp_servers.workbuddy]\ncommand = {}\nstartup_timeout_sec = 15\ntool_timeout_sec = 180\nenv = {{ WORKBUDDY_APP_DATA_DIR = {}, WORKBUDDY_DATABASE_PATH = {}, WORKBUDDY_SKILLS_DIRS = {}, WORKBUDDY_MARKETING_URL = {} }}\n",
            toml_string(&self.executable.display().to_string()),
            toml_string(&self.app_data_dir.display().to_string()),
            toml_string(&database_path.display().to_string()),
            toml_string(&skills),
            toml_string(&self.marketing_base_url),
        )
    }
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
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
    fn b1_version_has_semver_and_display_forms() {
        let config = CodexRuntimeConfig::b1_default();

        assert_eq!(config.package_version, "1.1.0-alpha.1");
        assert_eq!(config.display_version, "b1.0.5");
        assert_eq!(config.codex_version, "0.149.0");
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
}
