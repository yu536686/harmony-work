//! `robotz-browser` - Chrome-DevTools-Protocol web automation.
//!
//! [`BrowserTool`] (`browser`) drives a managed Chrome instance: navigate,
//! click, type, screenshot, Set-of-Mark element annotation, DOM interaction.
//! [`BrowserManager`] owns the Chrome-for-Testing lifecycle and page pool.
//!
//! Implements [`robotz_core::Tool`] and is exposed through MCP.

pub mod download;
pub mod manager;
pub mod snapshot;
pub mod tool;

pub use manager::{create_browser_manager, BrowserManager, BrowserOptions, SharedBrowserManager};
pub use tool::BrowserTool;
