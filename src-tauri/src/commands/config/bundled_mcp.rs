//! Legacy cleanup for bundled RobotZ MCP entries.
//!
//! Browser and desktop automation now use managed MCP servers. Older builds registered a `robotz` MCP sidecar in
//! settings — strip it on startup so the MCP tools page only lists external
//! servers the user configured.

use crate::store::settings::Settings;

/// Stable settings key used by legacy bundled MCP registration.
pub const ROBOTZ_MCP_SERVER_NAME: &str = "robotz";

/// Remove legacy `robotz` MCP config. Returns `true` if settings were mutated.
pub fn strip_legacy_robotz_mcp_server(settings: &mut Settings) -> bool {
    let before = settings.mcp_servers.len();
    settings
        .mcp_servers
        .retain(|s| s.name != ROBOTZ_MCP_SERVER_NAME);
    let removed = before != settings.mcp_servers.len();
    if removed {
        tracing::info!(
            "Removed legacy bundled MCP entry '{ROBOTZ_MCP_SERVER_NAME}' (replaced by managed automation MCP servers)"
        );
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::settings::McpServerConfig;

    #[test]
    fn strips_robotz_entry() {
        let mut settings = Settings::default();
        settings.mcp_servers.push(McpServerConfig {
            name: ROBOTZ_MCP_SERVER_NAME.into(),
            transport: "stdio".into(),
            command: "/tmp/robotz-mcp".into(),
            ..Default::default()
        });
        settings.mcp_servers.push(McpServerConfig {
            name: "custom".into(),
            transport: "sse".into(),
            url: "http://localhost".into(),
            ..Default::default()
        });
        assert!(strip_legacy_robotz_mcp_server(&mut settings));
        assert_eq!(settings.mcp_servers.len(), 1);
        assert_eq!(settings.mcp_servers[0].name, "custom");
    }
}
