//! Desktop-side StaffDeck persistence, events and Codex task dispatch.

pub mod bridge;
pub mod events;
pub mod notice;

pub use crate::agent_core::models::{
    KoiDefinition, KoiTodo, PoolMessage, PoolSession, StarterKoiSpec, KOI_COLORS, KOI_ICONS,
    STARTER_KOI_SPECS,
};
