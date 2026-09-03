use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy)]
pub struct ConfirmFlags {
    pub confirm_shell: bool,
    pub confirm_file_write: bool,
}

pub type ConfirmFlagsHandle = Arc<RwLock<ConfirmFlags>>;

pub fn confirm_flags_handle(confirm_shell: bool, confirm_file_write: bool) -> ConfirmFlagsHandle {
    Arc::new(RwLock::new(ConfirmFlags {
        confirm_shell,
        confirm_file_write,
    }))
}

pub fn sync_confirm_flags(
    handle: &ConfirmFlagsHandle,
    confirm_shell: bool,
    confirm_file_write: bool,
) {
    if let Ok(mut flags) = handle.write() {
        flags.confirm_shell = confirm_shell;
        flags.confirm_file_write = confirm_file_write;
    }
}
