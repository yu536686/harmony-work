#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "macos")]
    {
        let mut arguments = std::env::args_os().skip(1);
        if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--harmony-macos-update")) {
            let Some(transaction) = arguments.next() else {
                std::process::exit(2);
            };
            if arguments.next().is_some() {
                std::process::exit(2);
            }
            std::process::exit(xiaoheng_workbuddy_lib::updater::macos::run_helper(
                std::path::Path::new(&transaction),
            ));
        }
    }
    xiaoheng_workbuddy_lib::run();
}
