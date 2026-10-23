//! Cross-platform child process helpers that suppress console windows on Windows.

#[cfg(any(windows, target_os = "macos", test))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Directory owned by the offline bootstrapper. It keeps application runtimes
/// separate from user-managed Node, Python, and Git installations.
pub fn managed_runtime_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Harmony")
        .join("WorkBuddy")
        .join("runtime")
}

#[cfg(any(windows, test))]
fn managed_windows_runtime_executable_for_root(root: &Path, key: &str) -> Option<PathBuf> {
    let relative = match key {
        "node" => ["node", "node.exe"].as_slice(),
        "npm" => ["node", "npm.cmd"].as_slice(),
        "npx" => ["node", "npx.cmd"].as_slice(),
        "python" => ["python", "python.exe"].as_slice(),
        "pip" => ["python", "Scripts", "pip.exe"].as_slice(),
        "git" => ["git", "bin", "git.exe"].as_slice(),
        "wecom_cli" => ["wecom", "wecom-cli.cmd"].as_slice(),
        _ => return None,
    };
    Some(
        relative
            .iter()
            .fold(root.to_path_buf(), |path, segment| path.join(segment)),
    )
}

#[cfg(target_os = "macos")]
fn managed_macos_runtime_root_for_exe(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents")
        .then(|| contents.join("Resources").join("resources"))
}

#[cfg(target_os = "macos")]
fn managed_macos_runtime_executable_for_exe(exe: &Path, key: &str) -> Option<PathBuf> {
    let relative = match key {
        "node" => "node/bin/node",
        "npm" => "node/bin/npm",
        "npx" => "node/bin/npx",
        "python" => "python/bin/python3",
        "pip" => "python/bin/pip",
        "git" => "git/bin/git",
        "wecom_cli" => "wecom/bin/wecom-cli",
        _ => return None,
    };
    Some(managed_macos_runtime_root_for_exe(exe)?.join(relative))
}

#[cfg(target_os = "macos")]
fn managed_macos_runtime_root() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| managed_macos_runtime_root_for_exe(&exe));
    bundled.or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|cwd| cwd.join("src-tauri").join("resources"))
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn is_packaged_macos_app() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| managed_macos_runtime_root_for_exe(&exe))
        .is_some()
}

pub fn managed_runtime_executable(key: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let path = managed_windows_runtime_executable_for_root(&managed_runtime_root(), key)?;
        return path.exists().then_some(path);
    }
    #[cfg(target_os = "macos")]
    {
        let path = if let Ok(exe) = std::env::current_exe() {
            managed_macos_runtime_executable_for_exe(&exe, key)
        } else {
            None
        }
        .or_else(|| {
            let root = managed_macos_runtime_root()?;
            let relative = match key {
                "node" => "node/bin/node",
                "npm" => "node/bin/npm",
                "npx" => "node/bin/npx",
                "python" => "python/bin/python3",
                "pip" => "python/bin/pip",
                "git" => "git/bin/git",
                "wecom_cli" => "wecom/bin/wecom-cli",
                _ => return None,
            };
            Some(root.join(relative))
        })?;
        return path.is_file().then_some(path);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = key;
        None
    }
}

pub fn managed_runtime_bin_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let root = managed_runtime_root();
        return [
            root.join("node"),
            root.join("python"),
            root.join("python").join("Scripts"),
            root.join("git").join("bin"),
            root.join("wecom"),
        ]
        .into_iter()
        .filter(|path| path.exists())
        .collect();
    }
    #[cfg(target_os = "macos")]
    {
        let Some(root) = managed_macos_runtime_root() else {
            return Vec::new();
        };
        return ["node/bin", "python/bin", "git/bin", "wecom/bin"]
            .iter()
            .map(|relative| root.join(relative))
            .filter(|path| path.is_dir())
            .collect();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    Vec::new()
}

#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn managed_path_value() -> Option<std::ffi::OsString> {
    let mut paths = managed_runtime_bin_dirs();
    #[cfg(target_os = "macos")]
    paths.extend(
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
            .into_iter()
            .map(PathBuf::from),
    );
    if let Some(current_path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current_path));
    }
    std::env::join_paths(paths).ok()
}

#[allow(clippy::disallowed_methods)]
pub fn tokio_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = tokio::process::Command::new(program);
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        if let Some(path) = managed_path_value() {
            command.env("PATH", path);
        }
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = tokio::process::Command::new(program);
        #[cfg(target_os = "macos")]
        if let Some(path) = managed_path_value() {
            command.env("PATH", path);
        }
        command
    }
}

#[allow(clippy::disallowed_methods)]
pub fn std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        if let Some(path) = managed_path_value() {
            command.env("PATH", path);
        }
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new(program);
        #[cfg(target_os = "macos")]
        if let Some(path) = managed_path_value() {
            command.env("PATH", path);
        }
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_accept_a_program() {
        let _ = tokio_command("echo");
        let _ = std_command("echo");
    }

    #[test]
    fn managed_windows_runtime_layout_is_stable() {
        let root = PathBuf::from("C:/Users/test/AppData/Local/Harmony/WorkBuddy/runtime");
        assert_eq!(
            managed_windows_runtime_executable_for_root(&root, "node"),
            Some(root.join("node").join("node.exe"))
        );
        assert_eq!(
            managed_windows_runtime_executable_for_root(&root, "wecom_cli"),
            Some(root.join("wecom").join("wecom-cli.cmd"))
        );
        assert_eq!(
            managed_windows_runtime_executable_for_root(&root, "unknown"),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_macos_runtime_follows_the_app_bundle() {
        let exe = PathBuf::from("/Applications/Harmony Work.app/Contents/MacOS/harmony-work");
        let resources = PathBuf::from("/Applications/Harmony Work.app/Contents/Resources/resources");
        assert_eq!(
            managed_macos_runtime_executable_for_exe(&exe, "node"),
            Some(resources.join("node/bin/node"))
        );
        assert_eq!(
            managed_macos_runtime_executable_for_exe(&exe, "npm"),
            Some(resources.join("node/bin/npm"))
        );
        assert_eq!(
            managed_macos_runtime_executable_for_exe(&exe, "python"),
            Some(resources.join("python/bin/python3"))
        );
        assert_eq!(
            managed_macos_runtime_executable_for_exe(&exe, "git"),
            Some(resources.join("git/bin/git"))
        );
        assert_eq!(
            managed_macos_runtime_executable_for_exe(&exe, "wecom_cli"),
            Some(resources.join("wecom/bin/wecom-cli"))
        );
    }
}
