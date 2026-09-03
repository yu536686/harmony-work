//! User tool manifest discovery used by the settings UI.
//!
//! User tools are exposed to Codex through MCP. The desktop process only owns
//! installation and configuration metadata; it no longer executes them through
//! the legacy in-process Tool trait.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFieldSchema {
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: Option<String>,
    pub default: Option<Value>,
    pub description: Option<String>,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserToolManifest {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: String,
    pub runtime: String,
    pub entrypoint: String,
    pub input_schema: Value,
    #[serde(default)]
    pub config_schema: HashMap<String, ConfigFieldSchema>,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_version() -> String {
    "1.0".into()
}

fn default_timeout() -> u64 {
    60
}

impl UserToolManifest {
    pub fn load(dir: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(dir.join("manifest.json"))?;
        Ok(serde_json::from_str(&content)?)
    }

    #[allow(dead_code)]
    pub fn has_secret_fields(&self) -> bool {
        self.config_schema
            .values()
            .any(|field| field.field_type == "password")
    }
}

pub struct UserTool {
    pub manifest: UserToolManifest,
}

pub fn load_user_tools(user_tools_dir: &Path) -> Vec<UserTool> {
    let Ok(entries) = std::fs::read_dir(user_tools_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let tool_dir = entry.path();
            match UserToolManifest::load(&tool_dir) {
                Ok(manifest) => Some(UserTool { manifest }),
                Err(error) => {
                    tracing::warn!(path = %tool_dir.display(), %error, "invalid user tool manifest");
                    None
                }
            }
        })
        .collect()
}
