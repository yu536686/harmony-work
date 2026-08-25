pub use piscis_core::scene::{MemorySliceMode, SceneKind, ScenePolicy};

#[cfg(test)]
mod tests {
    use super::{SceneKind, ScenePolicy};
    use piscis_core::scene::{CollaborationContextMode, EventDigestMode};

    #[test]
    fn heartbeat_scene_policy_is_lightweight_and_disables_proactive_compaction() {
        let policy = ScenePolicy::for_kind(SceneKind::HeartbeatSupervisor);
        assert!(!policy.include_memory);
        assert!(!policy.include_task_state);
        assert!(policy.include_pool_context);
        assert_eq!(policy.auto_compact_threshold_override, Some(0));
    }

    #[test]
    fn collaboration_context_rules_still_come_from_core_policy() {
        assert_eq!(
            ScenePolicy::for_kind(SceneKind::MainChat).collaboration_context_mode(),
            CollaborationContextMode::OnDemand
        );
        assert_eq!(
            ScenePolicy::for_kind(SceneKind::HeartbeatSupervisor).event_digest_mode(),
            EventDigestMode::CoordinationPlusFailures
        );
    }

}
