pub const DISPLAY_VERSION: &str = "b1.0.0";
pub const PINNED_CODEX_VERSION: &str = "0.149.0";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b1_version_has_semver_and_display_forms() {
        let config = CodexRuntimeConfig::b1_default();

        assert_eq!(config.package_version, "1.0.0-beta.1");
        assert_eq!(config.display_version, "b1.0.0");
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
}
