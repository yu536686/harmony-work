#[allow(dead_code)] // All platform variants are exercised by cross-platform contract tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopPlatform {
    Windows,
    Macos,
    Linux,
}

impl DesktopPlatform {
    pub const fn current() -> Self {
        #[cfg(windows)]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Self::Linux
        }
    }

    pub const fn browser_channel(self) -> &'static str {
        match self {
            Self::Windows => "msedge",
            Self::Macos | Self::Linux => "chrome",
        }
    }

    pub const fn browser_download_url(self) -> &'static str {
        match self {
            Self::Windows => "https://www.microsoft.com/edge/download",
            Self::Macos | Self::Linux => "https://www.google.com/chrome/",
        }
    }

    pub const fn browser_missing_hint(self) -> &'static str {
        match self {
            Self::Windows => {
                "安装 Microsoft Edge 或 Google Chrome 后，重启 Harmony Work 以启用浏览器自动化。"
            }
            Self::Macos | Self::Linux => {
                "安装 Google Chrome 或 Microsoft Edge 后，重启 Harmony Work 以启用浏览器自动化。"
            }
        }
    }

    pub const fn shell_description(self) -> &'static str {
        match self {
            Self::Windows => "执行系统 Shell 命令（cmd.exe / bash）",
            Self::Macos => "执行 macOS zsh 命令",
            Self::Linux => "执行 Linux Shell 命令",
        }
    }

    pub const fn browser_description(self) -> &'static str {
        match self {
            Self::Windows => "通过 Playwright MCP 控制 Edge 或 Chrome，支持导航、点击、快照与截图；设置更改后需重启",
            Self::Macos | Self::Linux => "通过 Playwright MCP 控制 Chrome 或 Edge，支持导航、点击、快照与截图；设置更改后需重启",
        }
    }

    pub const fn computer_use_description(self) -> &'static str {
        match self {
            Self::Windows => {
                "通过 Windows UIA3 MCP 查看并操作桌面控件，必要时使用截图；设置更改后需重启"
            }
            Self::Macos => {
                "通过 macOS 辅助功能 MCP 查看并操作桌面控件，必要时使用截图；设置更改后需重启"
            }
            Self::Linux => "查看并操作桌面控件，必要时使用截图；设置更改后需重启",
        }
    }

    pub const fn uses_bundled_node(self) -> bool {
        matches!(self, Self::Macos)
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopPlatform;

    #[test]
    fn windows_defaults_remain_stable_on_every_test_host() {
        let platform = DesktopPlatform::Windows;
        assert_eq!(platform.browser_channel(), "msedge");
        assert_eq!(
            platform.browser_download_url(),
            "https://www.microsoft.com/edge/download"
        );
        assert!(platform.computer_use_description().contains("Windows UIA3"));
        assert!(!platform.uses_bundled_node());
    }

    #[test]
    fn macos_uses_its_own_browser_and_automation_profile() {
        let platform = DesktopPlatform::Macos;
        assert_eq!(platform.browser_channel(), "chrome");
        assert!(platform.computer_use_description().contains("macOS"));
        assert!(platform.uses_bundled_node());
    }
}
