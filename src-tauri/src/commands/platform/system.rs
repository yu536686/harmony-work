use crate::host::DesktopHostTools;
use crate::store::{AppState, Settings};
use piscis_kernel::proc::std_command;
use robotz_browser::download;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Serialize)]
pub struct VmStatus {
    pub backend: String,
    pub available: bool,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct RuntimeCapabilities {
    pub vm: VmStatus,
    pub tools: Vec<String>,
    pub channels: Vec<String>,
    pub configured_provider: String,
    pub workspace_root: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct RuntimeCheckItem {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub download_url: String,
    pub hint: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EnvironmentBootstrapItem {
    pub key: String,
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub local_installer: Option<String>,
    pub source: String,
    pub status: String,
    pub details: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct EnvironmentBootstrapResult {
    pub items: Vec<EnvironmentBootstrapItem>,
    pub logs: Vec<String>,
    pub workspace_root: String,
    pub offline_dirs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemDependencyItem {
    pub key: String,
    pub name: String,
    pub feature: String,
    pub available: bool,
    pub required: bool,
    pub status: String,
    pub details: Option<String>,
    pub hint: String,
    pub remediation: Option<String>,
    pub action: Option<SystemDependencyAction>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemDependencyAction {
    pub kind: String,
    pub command: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PrivilegeElevationCheckItem {
    pub key: String,
    pub name: String,
    pub available: bool,
    pub required: bool,
    pub status: String,
    pub details: Option<String>,
    pub hint: String,
    pub remediation: Option<String>,
    pub action: Option<SystemDependencyAction>,
}

/// Detect Node.js, npm, Python and other runtimes needed by skills.
/// `custom_paths` maps runtime key (e.g. "python") to an absolute executable path
/// supplied by the user when auto-detection fails.
#[tauri::command]
pub async fn check_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeCheckItem>, String> {
    let custom_paths = {
        let settings = state.settings.lock().await;
        settings.runtime_paths.clone()
    };

    let mut items = Vec::new();

    // Node.js
    let node = probe_with_override("node", &["--version"], &custom_paths)
        .or_else(|| probe_command("node", &["--version"]));
    items.push(RuntimeCheckItem {
        name: "Node.js".into(),
        available: node.is_some(),
        version: node,
        download_url: "https://nodejs.org/en/download".into(),
        hint: "Required for npm-based skills".into(),
    });

    // npm (bundled with Node, but check separately)
    let npm = probe_with_override("npm", &["--version"], &custom_paths)
        .or_else(|| probe_command("npm", &["--version"]));
    items.push(RuntimeCheckItem {
        name: "npm".into(),
        available: npm.is_some(),
        version: npm,
        download_url: "https://nodejs.org/en/download".into(),
        hint: "Package manager for Node.js skills".into(),
    });

    // Python
    let python = probe_with_override("python", &["--version"], &custom_paths)
        .or_else(|| probe_command("python", &["--version"]))
        .or_else(|| probe_command("python3", &["--version"]));
    items.push(RuntimeCheckItem {
        name: "Python".into(),
        available: python.is_some(),
        version: python,
        download_url: "https://www.python.org/downloads/".into(),
        hint: "Required for Python-based skills".into(),
    });

    // pip
    let pip = probe_with_override("pip", &["--version"], &custom_paths)
        .or_else(|| probe_command("pip", &["--version"]))
        .or_else(|| probe_command("pip3", &["--version"]));
    items.push(RuntimeCheckItem {
        name: "pip".into(),
        available: pip.is_some(),
        version: pip,
        download_url: "https://pip.pypa.io/en/stable/installation/".into(),
        hint: "Package manager for Python skills".into(),
    });

    // Git (needed for some skill installs)
    let git = probe_with_override("git", &["--version"], &custom_paths)
        .or_else(|| probe_command("git", &["--version"]));
    items.push(RuntimeCheckItem {
        name: "Git".into(),
        available: git.is_some(),
        version: git,
        download_url: "https://git-scm.com/downloads".into(),
        hint: "Required for git-based skill sources".into(),
    });

    // Browser (Chrome/Brave for browser automation tool)
    // Check system Chrome/Brave first, then cached Chrome for Testing
    let chrome_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.harmony.xiaoheng-workbuddy")
        .join("chrome");
    let (browser_available, browser_version, browser_hint) =
        if let Some(sys_path) = download::find_system_chrome() {
            let name = sys_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("chrome")
                .to_string();
            let ver = probe_command(sys_path.to_str().unwrap_or("chrome"), &["--version"])
                .unwrap_or_else(|| sys_path.to_string_lossy().to_string());
            (true, Some(ver), format!("System browser: {}", name))
        } else if let Some(cached) = download::chrome_exists(&chrome_dir) {
            let ver = probe_command(cached.to_str().unwrap_or(""), &["--version"])
                .unwrap_or_else(|| "Chrome for Testing (cached)".to_string());
            (
                true,
                Some(ver),
                "Chrome for Testing (cached, ~111 MB)".to_string(),
            )
        } else {
            (
                false,
                None,
                "No Chromium browser found. Chrome or Brave recommended. \
                 Chrome for Testing (~111 MB) will be auto-downloaded on first use."
                    .to_string(),
            )
        };
    items.push(RuntimeCheckItem {
        name: "Browser (Chrome/Brave)".into(),
        available: browser_available,
        version: browser_version,
        download_url: "https://www.google.com/chrome/".into(),
        hint: browser_hint,
    });

    Ok(items)
}

#[tauri::command]
pub async fn check_environment_bootstrap(
    state: State<'_, AppState>,
) -> Result<EnvironmentBootstrapResult, String> {
    let settings = state.settings.lock().await.clone();
    Ok(collect_environment_bootstrap(&settings, Vec::new()))
}

#[tauri::command]
pub async fn run_environment_bootstrap(
    state: State<'_, AppState>,
) -> Result<EnvironmentBootstrapResult, String> {
    let settings = state.settings.lock().await.clone();
    let mut logs = Vec::new();

    ensure_bootstrap_workspace(&settings, &mut logs);

    for target in bootstrap_targets() {
        install_bootstrap_target(target, &mut logs);
    }

    Ok(collect_environment_bootstrap(&settings, logs))
}

#[tauri::command]
pub async fn install_environment_bootstrap_item(
    key: String,
    state: State<'_, AppState>,
) -> Result<EnvironmentBootstrapResult, String> {
    let settings = state.settings.lock().await.clone();
    let mut logs = Vec::new();
    ensure_bootstrap_workspace(&settings, &mut logs);

    let target = bootstrap_targets()
        .into_iter()
        .find(|target| target.key == key)
        .ok_or_else(|| format!("Unknown environment bootstrap target: {key}"))?;

    install_bootstrap_target(target, &mut logs);
    Ok(collect_environment_bootstrap(&settings, logs))
}

/// Check platform-specific feature dependencies that affect desktop automation,
/// Windows integrations, and host runtime behavior.
#[tauri::command]
pub async fn check_system_dependencies(
    state: State<'_, AppState>,
) -> Result<Vec<SystemDependencyItem>, String> {
    let settings = state.settings.lock().await.clone();
    Ok(collect_system_dependencies(&settings))
}

#[tauri::command]
pub async fn run_system_dependency_action(key: String) -> Result<(), String> {
    let action = dependency_action_for_key(&key)
        .ok_or_else(|| format!("No assisted action available for dependency: {key}"))?;
    execute_dependency_action(&action)
}

#[tauri::command]
pub async fn check_privilege_elevation() -> Result<Vec<PrivilegeElevationCheckItem>, String> {
    Ok(collect_privilege_elevation_checks())
}

pub fn collect_system_dependencies(settings: &Settings) -> Vec<SystemDependencyItem> {
    let tool_enabled = |tool_name: &str| {
        settings
            .builtin_tool_enabled
            .get(tool_name)
            .copied()
            .unwrap_or(true)
    };

    let mut items = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let desktop_enabled = tool_enabled("desktop_automation");
        let session_type = std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_else(|_| "unknown".to_string())
            .to_lowercase();
        let x11 = session_type == "x11" || session_type.is_empty();

        items.push(build_dependency_item(
            "linux-session",
            "Display Session (X11)",
            "desktop_automation",
            x11,
            desktop_enabled,
            Some(format!("Current session: {}", if session_type.is_empty() { "unknown" } else { &session_type })),
            if x11 {
                "Desktop automation is running on an X11-compatible session."
            } else {
                "Wayland sessions often block synthetic mouse/keyboard input and window control."
            },
            Some("Use an X11 session for reliable desktop automation, or expect limited support under Wayland."),
            None,
        ));

        items.push(build_dependency_item(
            "xdotool",
            "xdotool",
            "desktop_automation",
            command_exists("xdotool"),
            desktop_enabled,
            None,
            "Mouse, keyboard, cursor position, dragging, and scroll automation on Linux.",
            Some("Install xdotool from your distro packages, e.g. `sudo apt install xdotool`."),
            dependency_action_for_key("xdotool"),
        ));

        items.push(build_dependency_item(
            "wmctrl",
            "wmctrl",
            "desktop_automation",
            command_exists("wmctrl"),
            desktop_enabled,
            None,
            "Window listing and window activation for desktop_automation on Linux.",
            Some("Install wmctrl from your distro packages, e.g. `sudo apt install wmctrl`."),
            dependency_action_for_key("wmctrl"),
        ));

        items.push(build_dependency_item(
            "xclip",
            "xclip",
            "desktop_automation",
            command_exists("xclip"),
            false,
            None,
            "Recommended for reliable clipboard-based text input; desktop_automation falls back to xdotool typing when missing.",
            Some("Install xclip for better text entry reliability, e.g. `sudo apt install xclip`."),
            dependency_action_for_key("xclip"),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let desktop_enabled = tool_enabled("desktop_automation");
        items.push(build_dependency_item(
            "cliclick",
            "cliclick",
            "desktop_automation",
            command_exists("cliclick"),
            desktop_enabled,
            None,
            "Mouse, keyboard, drag, and click automation on macOS.",
            Some("Install cliclick with Homebrew: `brew install cliclick`."),
            dependency_action_for_key("cliclick"),
        ));

        items.push(build_dependency_item(
            "osascript",
            "osascript",
            "desktop_automation",
            command_exists("osascript"),
            desktop_enabled,
            None,
            "Used for window listing, activation, and some fallback automation on macOS.",
            Some(
                "osascript ships with macOS. If unavailable, check shell PATH / system integrity.",
            ),
            None,
        ));

        let accessibility = probe_command(
            "osascript",
            &[
                "-e",
                "tell application \"System Events\" to return UI elements enabled",
            ],
        );
        let accessibility_ok = accessibility
            .as_deref()
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        items.push(build_dependency_item(
            "macos-accessibility",
            "Accessibility Permission",
            "desktop_automation",
            accessibility_ok,
            desktop_enabled,
            accessibility.map(|v| format!("System Events returned: {}", v)),
            "Required for controlling other apps via System Events / synthetic input.",
            Some("Grant Accessibility access to Harmony WorkBuddy / terminal in System Settings → Privacy & Security → Accessibility."),
            dependency_action_for_key("macos-accessibility"),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let desktop_enabled = tool_enabled("desktop_automation");
        let powershell_needed = desktop_enabled
            || tool_enabled("powershell_query")
            || tool_enabled("office")
            || tool_enabled("wmi");
        let powershell = command_exists("powershell") || command_exists("pwsh");

        items.push(build_dependency_item(
            "powershell",
            "PowerShell",
            "windows_integration",
            powershell,
            powershell_needed,
            probe_command("powershell", &["-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"])
                .or_else(|| probe_command("pwsh", &["-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"])),
            "Required by powershell_query and used by Windows desktop automation / Office helpers.",
            Some("Install Windows PowerShell / PowerShell 7 and ensure `powershell` or `pwsh` is available on PATH."),
            dependency_action_for_key("powershell"),
        ));

        if tool_enabled("uia") {
            items.push(build_dependency_item(
                "uia-runtime",
                "Windows UI Automation Runtime",
                "desktop_automation",
                true,
                true,
                Some("Built into the Windows desktop host".into()),
                "UIA support is compiled into the Windows build; failures are usually app-specific or permission-related.",
                Some("If UIA actions fail, try running the app with matching privilege level and verify the target app exposes UIA elements."),
                None,
            ));
        }

        if tool_enabled("wmi") {
            let wmi_status = probe_command(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "$svc=Get-Service Winmgmt -ErrorAction SilentlyContinue; if ($svc) { $svc.Status }",
                ],
            )
            .or_else(|| {
                probe_command(
                    "pwsh",
                    &[
                        "-NoProfile",
                        "-Command",
                        "$svc=Get-Service Winmgmt -ErrorAction SilentlyContinue; if ($svc) { $svc.Status }",
                    ],
                )
            });
            items.push(build_dependency_item(
                "wmi-service",
                "WMI Service (Winmgmt)",
                "windows_integration",
                wmi_status.is_some(),
                true,
                wmi_status.clone().map(|v| format!("Service status: {}", v)),
                "Needed for the WMI tool to query hardware, processes, and services.",
                Some("Ensure the Windows Management Instrumentation service exists and can be started."),
                dependency_action_for_key("wmi-service"),
            ));
        }

        if tool_enabled("office") {
            let office = probe_command(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "$paths=@('HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\excel.exe','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\winword.exe','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\powerpnt.exe'); if (($paths | Where-Object { Test-Path $_ }).Count -gt 0) { 'installed' }",
                ],
            )
            .or_else(|| {
                probe_command(
                    "pwsh",
                    &[
                        "-NoProfile",
                        "-Command",
                        "$paths=@('HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\excel.exe','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\winword.exe','HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\powerpnt.exe'); if (($paths | Where-Object { Test-Path $_ }).Count -gt 0) { 'installed' }",
                    ],
                )
            });
            items.push(build_dependency_item(
                "office-installation",
                "Microsoft Office",
                "office",
                office.is_some(),
                true,
                office.map(|v| format!("Registry probe: {}", v)),
                "Required for Excel / Word / PowerPoint / Outlook COM automation.",
                Some("Install Microsoft Office desktop apps on this machine before enabling the Office tool."),
                dependency_action_for_key("office-installation"),
            ));
        }
    }

    items
}

pub fn collect_privilege_elevation_checks() -> Vec<PrivilegeElevationCheckItem> {
    let mut items = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let pkexec_available = command_exists("pkexec");
        let graphical_session = std::env::var("DISPLAY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                std::env::var("WAYLAND_DISPLAY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            });
        let session_bus = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let polkit_agent = detect_linux_polkit_agent();

        items.push(build_privilege_item(
            "linux-pkexec",
            "pkexec",
            pkexec_available,
            true,
            None,
            "Needed for agent-triggered graphical root prompts on Linux.",
            Some("Install polkit / pkexec from your distro packages, or fall back to running sudo manually in a terminal."),
            dependency_action_for_key("linux-pkexec"),
        ));

        items.push(build_privilege_item(
            "linux-polkit-agent",
            "Polkit Authentication Agent",
            polkit_agent.is_some(),
            true,
            polkit_agent,
            "A desktop polkit agent must be available to show the graphical authentication dialog.",
            Some("Install a polkit authentication agent for your desktop environment, such as polkit-gnome, polkit-kde-agent, or lxqt-policykit."),
            None,
        ));

        items.push(build_privilege_item(
            "linux-graphical-session",
            "Graphical Session",
            graphical_session.is_some(),
            true,
            graphical_session.map(|value| format!("Detected display session: {value}")),
            "Graphical privilege prompts require a logged-in desktop session.",
            Some("Run Harmony WorkBuddy inside a normal desktop session; headless sessions and pure TTY shells cannot show privilege dialogs."),
            None,
        ));

        items.push(build_privilege_item(
            "linux-session-bus",
            "DBus Session Bus",
            session_bus.is_some(),
            true,
            session_bus.map(|value| format!("DBUS_SESSION_BUS_ADDRESS={value}")),
            "Polkit agents rely on the desktop session bus to coordinate authentication prompts.",
            Some("Start Harmony WorkBuddy from a normal desktop login session so DBUS_SESSION_BUS_ADDRESS is populated."),
            None,
        ));
    }

    #[cfg(target_os = "macos")]
    {
        items.push(build_privilege_item(
            "macos-admin-dialog",
            "Administrator Password Dialog",
            command_exists("osascript"),
            true,
            None,
            "Agent elevation uses AppleScript to trigger the native administrator password dialog.",
            Some("macOS ships with osascript. If the admin dialog cannot be shown, verify system integrity and run Harmony WorkBuddy from a normal desktop session."),
            None,
        ));
    }

    #[cfg(target_os = "windows")]
    {
        items.push(build_privilege_item(
            "windows-uac",
            "UAC Elevation Prompt",
            true,
            true,
            Some("Built into the Windows desktop host".into()),
            "Agent elevation uses the native UAC consent dialog on Windows.",
            Some("If elevation prompts fail, verify the account can approve UAC prompts and that desktop interaction is allowed."),
            None,
        ));
    }

    items
}

/// Save a user-specified runtime executable path and return updated check results.
#[tauri::command]
pub async fn set_runtime_path(
    state: State<'_, AppState>,
    runtime_key: String,
    exe_path: String,
) -> Result<Vec<RuntimeCheckItem>, String> {
    {
        let mut settings = state.settings.lock().await;
        if exe_path.is_empty() {
            settings.runtime_paths.remove(&runtime_key);
        } else {
            settings.runtime_paths.insert(runtime_key, exe_path);
        }
        settings.save().map_err(|e| e.to_string())?;
    }
    check_runtimes(state).await
}

/// Try to run the executable at the user-specified path (if any) for this runtime key.
fn probe_with_override(
    key: &str,
    args: &[&str],
    custom: &HashMap<String, String>,
) -> Option<String> {
    let path = custom.get(key)?;
    if path.is_empty() {
        return None;
    }
    probe_command(path, args)
}

fn build_dependency_item(
    key: &str,
    name: &str,
    feature: &str,
    available: bool,
    required: bool,
    details: Option<String>,
    hint: &str,
    remediation: Option<&str>,
    action: Option<SystemDependencyAction>,
) -> SystemDependencyItem {
    let status = if available {
        "ok"
    } else if required {
        "missing"
    } else {
        "warning"
    };

    SystemDependencyItem {
        key: key.to_string(),
        name: name.to_string(),
        feature: feature.to_string(),
        available,
        required,
        status: status.to_string(),
        details,
        hint: hint.to_string(),
        remediation: remediation.map(|s| s.to_string()),
        action,
    }
}

fn build_privilege_item(
    key: &str,
    name: &str,
    available: bool,
    required: bool,
    details: Option<String>,
    hint: &str,
    remediation: Option<&str>,
    action: Option<SystemDependencyAction>,
) -> PrivilegeElevationCheckItem {
    let status = if available {
        "ok"
    } else if required {
        "missing"
    } else {
        "warning"
    };

    PrivilegeElevationCheckItem {
        key: key.to_string(),
        name: name.to_string(),
        available,
        required,
        status: status.to_string(),
        details,
        hint: hint.to_string(),
        remediation: remediation.map(|s| s.to_string()),
        action,
    }
}

fn dependency_action_for_key(key: &str) -> Option<SystemDependencyAction> {
    #[cfg(target_os = "linux")]
    {
        return match key {
            "xdotool" => linux_install_action("xdotool"),
            "wmctrl" => linux_install_action("wmctrl"),
            "xclip" => linux_install_action("xclip"),
            "linux-pkexec" => linux_pkexec_install_action(),
            _ => None,
        };
    }

    #[cfg(target_os = "macos")]
    {
        return match key {
            "cliclick" => {
                if command_exists("brew") {
                    Some(SystemDependencyAction {
                        kind: "install_command".into(),
                        command: Some("brew install cliclick".into()),
                        url: None,
                    })
                } else {
                    Some(SystemDependencyAction {
                        kind: "open_url".into(),
                        command: None,
                        url: Some("https://brew.sh/".into()),
                    })
                }
            }
            "macos-accessibility" => Some(SystemDependencyAction {
                kind: "open_settings".into(),
                command: None,
                url: Some(
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                        .into(),
                ),
            }),
            _ => None,
        };
    }

    #[cfg(target_os = "windows")]
    {
        return match key {
            "powershell" => {
                if command_exists("winget") {
                    Some(SystemDependencyAction {
                        kind: "install_command".into(),
                        command: Some(
                            "winget install --id Microsoft.PowerShell --source winget".into(),
                        ),
                        url: None,
                    })
                } else {
                    Some(SystemDependencyAction {
                        kind: "open_url".into(),
                        command: None,
                        url: Some("https://github.com/PowerShell/PowerShell/releases".into()),
                    })
                }
            }
            "wmi-service" => Some(SystemDependencyAction {
                kind: "open_settings".into(),
                command: Some("services.msc".into()),
                url: None,
            }),
            "office-installation" => Some(SystemDependencyAction {
                kind: "open_url".into(),
                command: None,
                url: Some("https://www.microsoft.com/microsoft-365/download-office".into()),
            }),
            _ => None,
        };
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "linux")]
enum LinuxPackageManager {
    Apt,
    Dnf,
    Yum,
    Pacman,
    Zypper,
    Apk,
}

#[derive(Debug, Clone, Copy)]
struct BootstrapTarget {
    key: &'static str,
    name: &'static str,
    probe_command: &'static str,
    probe_args: &'static [&'static str],
    local_name_hints: &'static [&'static str],
    winget_id: Option<&'static str>,
}

fn bootstrap_targets() -> Vec<BootstrapTarget> {
    vec![
        BootstrapTarget {
            key: "git",
            name: "Git",
            probe_command: "git",
            probe_args: &["--version"],
            local_name_hints: &["git"],
            winget_id: Some("Git.Git"),
        },
        BootstrapTarget {
            key: "node",
            name: "Node.js LTS",
            probe_command: "node",
            probe_args: &["--version"],
            local_name_hints: &["node"],
            winget_id: Some("OpenJS.NodeJS.LTS"),
        },
        BootstrapTarget {
            key: "python",
            name: "Python",
            probe_command: "python",
            probe_args: &["--version"],
            local_name_hints: &["python"],
            winget_id: Some("Python.Python.3.12"),
        },
        BootstrapTarget {
            key: "wecom_cli",
            name: "WeCom CLI",
            probe_command: "wecom-cli",
            probe_args: &["--version"],
            local_name_hints: &["wecom", "@wecom", "cli"],
            winget_id: None,
        },
    ]
}

fn ensure_bootstrap_workspace(settings: &Settings, logs: &mut Vec<String>) {
    let workspace_root = if settings.workspace_root.trim().is_empty() {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("harmony")
    } else {
        PathBuf::from(&settings.workspace_root)
    };
    match std::fs::create_dir_all(&workspace_root) {
        Ok(_) => logs.push(format!("Workspace ready: {}", workspace_root.display())),
        Err(err) => logs.push(format!(
            "Workspace create failed: {} ({err})",
            workspace_root.display()
        )),
    }
}

fn install_bootstrap_target(target: BootstrapTarget, logs: &mut Vec<String>) {
    if target_available(target).is_some() {
        logs.push(format!("{} already available.", target.name));
        return;
    }

    if let Some(installer) = find_local_installer(target) {
        logs.push(format!(
            "Installing {} from local package: {}",
            target.name,
            installer.display()
        ));
        match run_local_installer(target, &installer) {
            Ok(msg) => logs.push(msg),
            Err(err) => logs.push(format!("{} local install failed: {err}", target.name)),
        }
        return;
    }

    if let Some(command) = online_install_command(target) {
        logs.push(format!(
            "No local installer for {}; trying: {command}",
            target.name
        ));
        match run_shell_command(&command) {
            Ok(msg) => logs.push(msg),
            Err(err) => logs.push(format!("{} online install failed: {err}", target.name)),
        }
    } else {
        logs.push(format!(
            "No local installer and no assisted online installer for {}.",
            target.name
        ));
    }
}

fn collect_environment_bootstrap(
    settings: &Settings,
    logs: Vec<String>,
) -> EnvironmentBootstrapResult {
    let items = bootstrap_targets()
        .into_iter()
        .map(|target| {
            let version = target_available(target);
            let local = find_local_installer(target);
            let source = if version.is_some() {
                "system".to_string()
            } else if local.is_some() {
                "local".to_string()
            } else if target.winget_id.is_some() || target.key == "wecom_cli" {
                "online".to_string()
            } else {
                "missing".to_string()
            };
            let details = if version.is_some() {
                "Installed and ready.".to_string()
            } else if let Some(path) = &local {
                format!("Local installer found: {}", path.display())
            } else if target.key == "wecom_cli" {
                "No local @wecom/cli tgz package found; online fallback will try npm install -g @wecom/cli."
                    .to_string()
            } else {
                "No local installer found; online fallback will try silent winget install.".to_string()
            };
            EnvironmentBootstrapItem {
                key: target.key.to_string(),
                name: target.name.to_string(),
                available: version.is_some(),
                version,
                local_installer: local.map(|path| path.to_string_lossy().to_string()),
                source,
                status: if target_available(target).is_some() {
                    "ready".to_string()
                } else {
                    "missing".to_string()
                },
                details,
            }
        })
        .collect::<Vec<_>>();

    let workspace_root = if settings.workspace_root.trim().is_empty() {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("harmony")
            .to_string_lossy()
            .to_string()
    } else {
        settings.workspace_root.clone()
    };

    EnvironmentBootstrapResult {
        items,
        logs,
        workspace_root,
        offline_dirs: offline_installer_dirs()
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
    }
}

fn target_available(target: BootstrapTarget) -> Option<String> {
    if target.key == "wecom_cli" {
        return probe_command(target.probe_command, target.probe_args)
            .or_else(|| probe_command("wecom-cli.cmd", target.probe_args))
            .or_else(probe_wecom_cli_from_npm_prefix)
            .or_else(|| probe_command("npx", &["-y", "@wecom/cli", "--version"]))
            .or_else(|| probe_command("npx.cmd", &["-y", "@wecom/cli", "--version"]))
            .or_else(|| probe_command("npm", &["list", "-g", "@wecom/cli", "--depth=0"]))
            .or_else(|| probe_command("npm.cmd", &["list", "-g", "@wecom/cli", "--depth=0"]));
    }
    probe_command(target.probe_command, target.probe_args)
}

fn probe_wecom_cli_from_npm_prefix() -> Option<String> {
    let prefix = probe_command("npm", &["prefix", "-g"])
        .or_else(|| probe_command("npm.cmd", &["prefix", "-g"]))?;
    let prefix = PathBuf::from(prefix.trim());

    #[cfg(target_os = "windows")]
    let candidates = vec![prefix.join("wecom-cli.cmd"), prefix.join("wecom-cli")];

    #[cfg(not(target_os = "windows"))]
    let candidates = vec![prefix.join("bin").join("wecom-cli")];

    candidates
        .into_iter()
        .filter(|path| path.exists())
        .find_map(|path| probe_command(path.to_string_lossy().as_ref(), &["--version"]))
}

fn offline_installer_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("installers"));
            dirs.push(parent.join("offline-installers"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("installers"));
        dirs.push(cwd.join("offline-installers"));
        dirs.push(
            cwd.join("src-tauri")
                .join("resources")
                .join("offline-installers"),
        );
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn find_local_installer(target: BootstrapTarget) -> Option<PathBuf> {
    let allowed_exts = if target.key == "wecom_cli" {
        vec!["tgz", "tar"]
    } else {
        vec!["exe", "msi"]
    };
    for dir in offline_installer_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !allowed_exts.iter().any(|candidate| candidate == &ext) {
                continue;
            }
            if target
                .local_name_hints
                .iter()
                .all(|hint| file_name.contains(&hint.to_ascii_lowercase()))
                || target
                    .local_name_hints
                    .iter()
                    .any(|hint| file_name.contains(&hint.to_ascii_lowercase()))
            {
                return Some(path);
            }
        }
    }
    None
}

fn run_local_installer(target: BootstrapTarget, path: &Path) -> Result<String, String> {
    if target.key == "wecom_cli" {
        let command = format!("npm install -g \"{}\"", path.display());
        return run_shell_command(&command);
    }

    #[cfg(target_os = "windows")]
    {
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut command = if ext == "msi" {
            let mut cmd = std_command("msiexec");
            cmd.args(["/i", &path.to_string_lossy()]);
            cmd
        } else {
            std_command(path.as_os_str())
        };
        let output = command
            .output()
            .map_err(|err| format!("Failed to start local installer: {err}"))?;
        if output.status.success() {
            Ok(format!("{} local installer completed.", target.name))
        } else {
            Err(format!(
                "installer exit={:?}; stdout={}; stderr={}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(format!(
            "Local installer execution is only automated on Windows: {}",
            path.display()
        ))
    }
}

fn online_install_command(target: BootstrapTarget) -> Option<String> {
    if target.key == "wecom_cli" {
        if command_exists("npm") {
            return Some("npm install -g @wecom/cli".to_string());
        }
        return None;
    }
    let winget_id = target.winget_id?;
    if command_exists("winget") {
        Some(format!(
            "winget install --id {winget_id} -e --silent --accept-source-agreements --accept-package-agreements"
        ))
    } else {
        None
    }
}

fn run_shell_command(command: &str) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    let mut process = {
        let mut cmd = std_command("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            command,
        ]);
        cmd
    };

    #[cfg(not(target_os = "windows"))]
    let mut process = {
        let mut cmd = std_command("sh");
        cmd.args(["-lc", command]);
        cmd
    };

    let output = process
        .output()
        .map_err(|err| format!("Failed to run command: {err}"))?;
    if output.status.success() {
        Ok(format!(
            "Command completed: {}\n{}",
            command,
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    } else {
        Err(format!(
            "command exit={:?}; stdout={}; stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[cfg(target_os = "linux")]
fn linux_install_action(package: &str) -> Option<SystemDependencyAction> {
    detect_linux_package_manager().map(|manager| SystemDependencyAction {
        kind: "install_command".into(),
        command: Some(match manager {
            LinuxPackageManager::Apt => format!("sudo apt install -y {package}"),
            LinuxPackageManager::Dnf => format!("sudo dnf install -y {package}"),
            LinuxPackageManager::Yum => format!("sudo yum install -y {package}"),
            LinuxPackageManager::Pacman => format!("sudo pacman -S --needed {package}"),
            LinuxPackageManager::Zypper => format!("sudo zypper install -y {package}"),
            LinuxPackageManager::Apk => format!("sudo apk add {package}"),
        }),
        url: None,
    })
}

#[cfg(target_os = "linux")]
fn linux_pkexec_install_action() -> Option<SystemDependencyAction> {
    detect_linux_package_manager().map(|manager| {
        let package = match manager {
            LinuxPackageManager::Apt => "policykit-1",
            LinuxPackageManager::Dnf => "polkit",
            LinuxPackageManager::Yum => "polkit",
            LinuxPackageManager::Pacman => "polkit",
            LinuxPackageManager::Zypper => "polkit",
            LinuxPackageManager::Apk => "polkit",
        };

        SystemDependencyAction {
            kind: "install_command".into(),
            command: Some(match manager {
                LinuxPackageManager::Apt => format!("sudo apt install -y {package}"),
                LinuxPackageManager::Dnf => format!("sudo dnf install -y {package}"),
                LinuxPackageManager::Yum => format!("sudo yum install -y {package}"),
                LinuxPackageManager::Pacman => format!("sudo pacman -S --needed {package}"),
                LinuxPackageManager::Zypper => format!("sudo zypper install -y {package}"),
                LinuxPackageManager::Apk => format!("sudo apk add {package}"),
            }),
            url: None,
        }
    })
}

#[cfg(target_os = "linux")]
fn detect_linux_package_manager() -> Option<LinuxPackageManager> {
    if command_exists("apt") {
        Some(LinuxPackageManager::Apt)
    } else if command_exists("dnf") {
        Some(LinuxPackageManager::Dnf)
    } else if command_exists("yum") {
        Some(LinuxPackageManager::Yum)
    } else if command_exists("pacman") {
        Some(LinuxPackageManager::Pacman)
    } else if command_exists("zypper") {
        Some(LinuxPackageManager::Zypper)
    } else if command_exists("apk") {
        Some(LinuxPackageManager::Apk)
    } else {
        None
    }
}

fn execute_dependency_action(action: &SystemDependencyAction) -> Result<(), String> {
    match action.kind.as_str() {
        "install_command" => {
            let command = action
                .command
                .as_deref()
                .ok_or_else(|| "Missing install command".to_string())?;
            launch_terminal_command(command)
        }
        "open_url" => {
            let url = action
                .url
                .as_deref()
                .ok_or_else(|| "Missing URL".to_string())?;
            open_with_system(url)
        }
        "open_settings" => {
            #[cfg(target_os = "windows")]
            {
                let command = action.command.as_deref().unwrap_or("services.msc");
                let mut process = std_command("cmd");
                process
                    .args(["/C", "start", "", command])
                    .spawn()
                    .map_err(|e| format!("Failed to open settings: {e}"))?;
                Ok(())
            }
            #[cfg(not(target_os = "windows"))]
            {
                let url = action
                    .url
                    .as_deref()
                    .ok_or_else(|| "Missing settings target".to_string())?;
                open_with_system(url)
            }
        }
        other => Err(format!("Unsupported dependency action: {other}")),
    }
}

fn open_with_system(target: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let mut process = std_command("cmd");
        process
            .args(["/C", "start", "", target])
            .spawn()
            .map_err(|e| format!("Failed to open target: {e}"))?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std_command(cmd)
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to open target: {e}"))?;
        Ok(())
    }
}

fn launch_terminal_command(command: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let terminal_candidates: [(&str, &[&str]); 5] = [
            ("x-terminal-emulator", &["-e"]),
            ("gnome-terminal", &["--", "bash", "-lc"]),
            ("konsole", &["-e", "bash", "-lc"]),
            ("xfce4-terminal", &["--command"]),
            ("xterm", &["-e"]),
        ];

        for (terminal, prefix) in terminal_candidates {
            if !command_exists(terminal) {
                continue;
            }

            let mut process = std_command(terminal);
            process.args(prefix);
            if terminal == "xfce4-terminal" {
                process.arg(format!(
                    "bash -lc \"{}; exec bash\"",
                    shell_escape_double(command)
                ));
            } else {
                process.arg(format!("{}; exec bash", command));
            }

            if process.spawn().is_ok() {
                return Ok(());
            }
        }

        Err("No supported terminal emulator found to run the install command".into())
    }

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Terminal\" to do script \"{}\"\nactivate",
            apple_script_escape(&format!("{}; exec $SHELL", command))
        );
        std_command("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| format!("Failed to open Terminal.app: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        let mut process = std_command("powershell");
        process
            .args(["-NoProfile", "-NonInteractive", "-Command", command])
            .spawn()
            .map_err(|e| format!("Failed to run install command in background: {e}"))?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn shell_escape_double(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn apple_script_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std_command("where");
        command.arg(cmd);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = std_command("which");
        command.arg(cmd);
        command
    };

    command
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn probe_command(cmd: &str, args: &[&str]) -> Option<String> {
    let mut command = std_command(cmd);
    command.args(args);
    let output = command.output().ok()?;
    if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Some tools (like Python) print to stderr
        let raw = if raw.is_empty() {
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        } else {
            raw
        };
        Some(raw)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_polkit_agent() -> Option<String> {
    if let Some(process) = probe_command(
        "sh",
        &[
            "-c",
            "command -v pgrep >/dev/null 2>&1 && pgrep -fa 'polkit|policykit|authentication-agent' | head -n 1",
        ],
    )
    .filter(|value| !value.trim().is_empty())
    {
        return Some(format!("Running agent process: {process}"));
    }

    let candidates = [
        (
            "polkit-gnome-authentication-agent-1",
            [
                "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1",
                "/usr/libexec/polkit-gnome-authentication-agent-1",
            ],
        ),
        (
            "polkit-kde-authentication-agent-1",
            [
                "/usr/libexec/polkit-kde-authentication-agent-1",
                "/usr/lib/x86_64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
            ],
        ),
        (
            "lxqt-policykit-agent",
            [
                "/usr/bin/lxqt-policykit-agent",
                "/usr/libexec/lxqt-policykit-agent",
            ],
        ),
        (
            "mate-polkit",
            [
                "/usr/lib/mate-polkit/polkit-mate-authentication-agent-1",
                "/usr/libexec/polkit-mate-authentication-agent-1",
            ],
        ),
    ];

    for (binary, paths) in candidates {
        if command_exists(binary) {
            return Some(format!("Found agent binary on PATH: {binary}"));
        }
        for path in paths {
            if std::path::Path::new(path).exists() {
                return Some(format!("Found agent binary: {path}"));
            }
        }
    }

    None
}

/// Returns the current execution mode (always "host" in this Rust implementation)
#[tauri::command]
pub async fn get_vm_status() -> Result<VmStatus, String> {
    #[cfg(target_os = "windows")]
    {
        // Check if Windows Sandbox is available
        let sandbox_available =
            std::path::Path::new(r"C:\Windows\System32\WindowsSandbox.exe").exists();

        if sandbox_available {
            return Ok(VmStatus {
                backend: "windows_sandbox".into(),
                available: true,
                description: "Windows Sandbox available (optional isolation)".into(),
            });
        }
    }

    Ok(VmStatus {
        backend: "host".into(),
        available: true,
        description: "Running directly on host with Policy Gate security".into(),
    })
}

/// Runtime snapshot for parity tracking and diagnostics.
#[tauri::command]
pub async fn get_runtime_capabilities(
    state: State<'_, AppState>,
) -> Result<RuntimeCapabilities, String> {
    let vm = get_vm_status().await?;

    let settings = state.settings.lock().await.clone();
    let registry = DesktopHostTools {
        browser: Some(state.browser.clone()),
        builtin_tool_enabled: Some(settings.builtin_tool_enabled.clone()),
        ..Default::default()
    }
    .build_registry();
    let tools = registry
        .all()
        .iter()
        .map(|t| t.name().to_string())
        .collect::<Vec<_>>();

    let channels = state
        .gateway
        .list_channels()
        .await
        .into_iter()
        .map(|c| c.name)
        .collect::<Vec<_>>();

    Ok(RuntimeCapabilities {
        vm,
        tools,
        channels,
        configured_provider: settings.provider,
        workspace_root: settings.workspace_root,
    })
}
