//! Validate external-open targets before handing them to the OS, never a shell.
use std::path::Path;

fn validated_target(input: &str) -> Result<String, String> {
    if input.is_empty() || input.chars().any(char::is_control) {
        return Err("Empty target or control character in open target".into());
    }
    let drive_path = cfg!(windows)
        && input.as_bytes().get(1) == Some(&b':')
        && input.as_bytes()[0].is_ascii_alphabetic();
    if !drive_path {
        if let Ok(url) = reqwest::Url::parse(input) {
            return match url.scheme() {
                "http" | "https" if url.host_str().is_some() => Ok(url.to_string()),
                _ => Err("Only http/https URLs and existing local paths may be opened".into()),
            };
        }
        if input.contains("://") {
            return Err("Invalid URL".into());
        }
    }
    if input.starts_with(['/', '\\']) && input.replace('\\', "/").starts_with("//") {
        return Err("Network and device paths may not be opened".into());
    }
    let path = Path::new(input)
        .canonicalize()
        .map_err(|e| format!("Invalid local path: {e}"))?;
    let metadata = path.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("Only local files and directories may be opened".into());
    }
    let target = path.to_str().ok_or("Local path is not valid Unicode")?;
    #[cfg(windows)]
    {
        // canonicalize adds the extended DOS prefix; ShellExecute expects a DOS path.
        let target = target.strip_prefix("\\\\?\\").unwrap_or(target);
        if target.starts_with("\\\\") || target.starts_with("UNC\\") {
            return Err("Network and device paths may not be opened".into());
        }
        return Ok(target.to_string());
    }
    #[cfg(not(windows))]
    Ok(target.to_string())
}

pub(super) fn open(input: &str) -> Result<(), String> {
    let target = validated_target(input)?;
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        // No parameters, command line, interpreter or string interpolation.
        let result = unsafe {
            ShellExecuteW(
                HWND::default(),
                PCWSTR(verb.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        let code = result.0 as isize;
        if code <= 32 {
            return Err(format!(
                "Windows failed to open target (ShellExecuteW: {code})"
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        crate::process::std_command(program)
            .arg(target)
            .spawn()
            .map_err(|e| format!("Failed to open target: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_external_open_validates_schemes_and_literals() {
        for target in [
            "javascript:alert(1)",
            "file:///tmp/a",
            "mailto:a@b.com",
            "shell:AppsFolder",
            "ms-settings:display",
            "https://",
            "https://example.com/\ncalc",
            "\\\\server\\share",
            "\\\\?\\C:\\x",
            "",
        ] {
            assert!(validated_target(target).is_err(), "accepted {target:?}");
        }
        let url = "https://example.com/中文?q=a&x=(b)%25";
        assert!(validated_target(url)
            .unwrap()
            .starts_with("https://example.com/"));
        let dir = std::env::temp_dir().join(format!("harmony-open-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("中文 空格 & (x) % ! ^ ;.txt");
        std::fs::write(&file, "sentinel").unwrap();
        assert!(validated_target(file.to_str().unwrap()).is_ok());
        assert!(validated_target(dir.to_str().unwrap()).is_ok());
        assert!(validated_target(dir.join("missing").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
