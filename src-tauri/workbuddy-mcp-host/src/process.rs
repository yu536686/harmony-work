//! Cross-platform child process helpers that suppress console windows on Windows.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[allow(clippy::disallowed_methods)]
pub fn tokio_command<S: AsRef<std::ffi::OsStr>>(program: S) -> tokio::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = tokio::process::Command::new(program);
        command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    tokio::process::Command::new(program)
}

#[allow(clippy::disallowed_methods)]
pub fn std_command<S: AsRef<std::ffi::OsStr>>(program: S) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_accept_a_program() {
        let _ = tokio_command("echo");
        let _ = std_command("echo");
    }
}
