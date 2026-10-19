#![recursion_limit = "512"]

// lib.rs — Tauri application library entry point.
// main.rs calls run() from here; this allows Tauri mobile targets to work.

pub mod agent_core;
pub mod app;
mod browser_runtime;
pub mod codex_runtime;
mod commands;
mod coordination;
mod file_journal;
mod fish;
mod gateway;
#[path = "../shared/harvis_responses.rs"]
mod harvis_responses;
mod harmony_cut;
mod harvis_prompt;
pub mod headless_cli;
pub mod lsp;
mod mac_native_shell;
pub mod notify;
pub mod pool;
pub mod process;
mod platform_profile;
pub mod scheduler;
mod skills;
pub mod store;
#[path = "../shared/service_urls.rs"]
pub mod service_urls;
mod tools;
pub mod updater;

pub use app::run;
pub use store::AppState;
