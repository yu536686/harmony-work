#![recursion_limit = "512"]

// lib.rs — Tauri application library entry point.
// main.rs calls run() from here; this allows Tauri mobile targets to work.

pub mod agent_core;
pub mod app;
pub mod codex_runtime;
mod commands;
mod coordination;
mod file_journal;
mod fish;
mod gateway;
mod harmony_cut;
mod harvis_prompt;
pub mod headless_cli;
pub mod lsp;
pub mod notify;
pub mod pool;
pub mod process;
pub mod scheduler;
mod skills;
pub mod store;
mod tools;

pub use app::run;
pub use store::AppState;
