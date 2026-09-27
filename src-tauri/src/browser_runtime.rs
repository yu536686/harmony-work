use std::path::{Path, PathBuf};

pub struct BrowserInstallation {
    pub channel: &'static str,
    pub name: &'static str,
    pub executable: PathBuf,
}

pub fn installed_browser() -> Option<BrowserInstallation> {
    #[cfg(windows)]
    {
        detect_browser_from_roots(
            std::env::var_os("ProgramFiles(x86)").as_deref().map(Path::new),
            std::env::var_os("ProgramFiles").as_deref().map(Path::new),
            std::env::var_os("LOCALAPPDATA").as_deref().map(Path::new),
        )
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos_browser(
            std::env::var_os("HOME").as_deref().map(Path::new),
            Path::new("/Applications"),
        )
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn detect_macos_browser(home: Option<&Path>, applications: &Path) -> Option<BrowserInstallation> {
    let user_apps = home.map(|path| path.join("Applications"));
    let candidates = [
        (applications.join("Google Chrome.app/Contents/MacOS/Google Chrome"), "chrome", "Google Chrome"),
        (applications.join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"), "msedge", "Microsoft Edge"),
        (user_apps.as_deref().unwrap_or(applications).join("Google Chrome.app/Contents/MacOS/Google Chrome"), "chrome", "Google Chrome"),
        (user_apps.as_deref().unwrap_or(applications).join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"), "msedge", "Microsoft Edge"),
    ];
    candidates.into_iter().find_map(|(executable, channel, name)| {
        executable.is_file().then_some(BrowserInstallation { channel, name, executable })
    })
}

#[cfg(any(windows, test))]
fn detect_browser_from_roots(
    program_files_x86: Option<&Path>,
    program_files: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Option<BrowserInstallation> {
    let candidates = [
        (
            program_files_x86,
            "Microsoft/Edge/Application/msedge.exe",
            "msedge",
            "Microsoft Edge",
        ),
        (
            program_files,
            "Microsoft/Edge/Application/msedge.exe",
            "msedge",
            "Microsoft Edge",
        ),
        (
            local_app_data,
            "Microsoft/Edge/Application/msedge.exe",
            "msedge",
            "Microsoft Edge",
        ),
        (
            program_files,
            "Google/Chrome/Application/chrome.exe",
            "chrome",
            "Google Chrome",
        ),
        (
            program_files_x86,
            "Google/Chrome/Application/chrome.exe",
            "chrome",
            "Google Chrome",
        ),
        (
            local_app_data,
            "Google/Chrome/Application/chrome.exe",
            "chrome",
            "Google Chrome",
        ),
    ];
    candidates.into_iter().find_map(|(root, relative, channel, name)| {
        let executable = root?.join(relative);
        executable.is_file().then_some(BrowserInstallation {
            channel,
            name,
            executable,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{detect_browser_from_roots, detect_macos_browser};

    fn test_root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("harmony-browser-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn prefers_edge_and_falls_back_to_chrome() {
        let root = test_root();
        let files = root.join("Program Files");
        let edge = files.join("Microsoft/Edge/Application/msedge.exe");
        let chrome = files.join("Google/Chrome/Application/chrome.exe");
        std::fs::create_dir_all(edge.parent().unwrap()).unwrap();
        std::fs::create_dir_all(chrome.parent().unwrap()).unwrap();
        std::fs::write(&chrome, "").unwrap();

        let browser = detect_browser_from_roots(None, Some(&files), None).unwrap();
        assert_eq!(browser.channel, "chrome");

        std::fs::write(&edge, "").unwrap();
        let browser = detect_browser_from_roots(None, Some(&files), None).unwrap();
        assert_eq!(browser.channel, "msedge");
        assert_eq!(browser.executable, edge);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_no_browser_when_candidates_are_missing() {
        let root = test_root();
        assert!(detect_browser_from_roots(None, Some(&root), None).is_none());
    }

    #[test]
    fn discovers_macos_chrome_in_applications() {
        let root = test_root();
        let apps = root.join("Applications");
        let chrome = apps.join("Google Chrome.app/Contents/MacOS/Google Chrome");
        std::fs::create_dir_all(chrome.parent().unwrap()).unwrap();
        std::fs::write(&chrome, "").unwrap();
        let browser = detect_macos_browser(Some(&root), &apps).unwrap();
        assert_eq!(browser.channel, "chrome");
        assert_eq!(browser.executable, chrome);
        std::fs::remove_dir_all(root).unwrap();
    }
}
