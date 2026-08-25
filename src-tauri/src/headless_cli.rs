//! Desktop-side helpers for headless execution.
//!
//! Canonical schemas live in [`piscis_core::host`]. This module only owns
//! the pieces that require desktop-specific knowledge:
//!
//!   * [`disabled_tools_for_mode`] — computed from the desktop tool profile.

pub use piscis_core::host::{
    DisabledToolInfo, HeadlessCliMode, HeadlessCliRequest, HeadlessCliResponse,
    HeadlessContextToggles, PoolWaitSummary,
};

pub fn disabled_tools_for_mode(_mode: HeadlessCliMode) -> Vec<DisabledToolInfo> {
    Vec::new()
}
