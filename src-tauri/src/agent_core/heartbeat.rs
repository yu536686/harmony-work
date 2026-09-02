use super::models::{KoiTodo, PoolMessage, PoolSession};
use super::project_state::{
    assess_project_state, contains_coordinator_mention, contains_delegated_coordinator_mention,
    extract_project_status_signal, ProjectAssessment, ProjectDecision,
};

#[derive(Debug, Clone)]
pub struct PoolAttention {
    pub pool_id: String,
    pub pool_name: String,
    pub latest_message_id: i64,
    pub session_id: String,
    pub summary: String,
    pub assessment: ProjectAssessment,
    /// Truncated org_spec text for convergence checks (data injection, not flow rules).
    pub org_spec_excerpt: Option<String>,
}

/// True when the host assessment says Xiaoheng should coordinate, not merely ack.
pub fn assessment_requires_coordination(assessment: &ProjectAssessment) -> bool {
    !matches!(assessment.decision, ProjectDecision::Continue)
        || assessment.needs_review_count > 0
        || assessment.blocked_todo_count > 0
}

/// Detect heartbeat turns that ended without observable pool coordination.
pub fn is_heartbeat_ack_only(response: &str) -> bool {
    let trimmed = response.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_lowercase();
    let short = trimmed.chars().count() <= 240;
    short
        && (lower.contains("heartbeat_ok")
            || lower.contains("无需干预")
            || lower.contains("no intervention")
            || lower.contains("no change"))
}

/// State-derived supervisor note when a heartbeat turn did not touch the pool.
pub fn build_heartbeat_coordination_gap_notice(attention: &PoolAttention) -> String {
    let reasons = if attention.assessment.attention_reasons.is_empty() {
        attention.assessment.summary.clone()
    } else {
        attention.assessment.attention_reasons.join("; ")
    };
    format!(
        "[Heartbeat patrol · state trigger]\n\
         Decision: {:?}\n\
         Assessment: {}\n\
         Attention: {}\n\
         This note was posted because the heartbeat turn did not produce observable project coordination. \
         Compare org_spec against StaffDeck task evidence and use staff_task_create or staff_task_update as appropriate.",
        attention.assessment.decision,
        attention.assessment.summary,
        reasons,
    )
}

fn preview_chars(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    format!("{}...", content.chars().take(max_chars).collect::<String>())
}

fn is_attention_event(msg: &PoolMessage, koi_ids: &[String]) -> bool {
    if matches!(msg.sender_id.as_str(), "xiaoheng" | "piscis") {
        return false;
    }
    let from_known_koi = koi_ids.iter().any(|id| id == &msg.sender_id);
    if contains_coordinator_mention(&msg.content) {
        return true;
    }
    if from_known_koi && extract_project_status_signal(&msg.content).is_some() {
        return true;
    }
    matches!(
        msg.event_type.as_deref(),
        Some(
            "task_completed"
                | "task_failed"
                | "task_claimed"
                | "task_blocked"
                | "task_cancelled"
                | "protocol_reminder"
                | "task_progress"
        )
    )
}

pub fn pool_coordinator_session_id(pool_id: &str) -> String {
    format!("xiaoheng_pool_{}", pool_id)
}

pub fn build_pool_heartbeat_message(base_prompt: &str, attention: &PoolAttention) -> String {
    let assessment = &attention.assessment;
    let mut lines = vec![
        base_prompt.to_string(),
        String::new(),
        "## Heartbeat Inbox".to_string(),
        attention.summary.clone(),
        String::new(),
        "## Current Project State".to_string(),
        "- The fields below are a host-generated snapshot of observable state, not a final project judgment.".to_string(),
        format!("- Decision: {:?}", assessment.decision),
        format!("- Active todos: {}", assessment.active_todo_count),
        format!("- Blocked todos: {}", assessment.blocked_todo_count),
        format!("- Needs-review todos: {}", assessment.needs_review_count),
        format!("- Integration-ready branches: {}", assessment.integration_ready_count),
        format!("- Dependency-blocked todos: {}", assessment.dependency_blocked_count),
        format!("- task_failed events: {}", assessment.task_failed_count),
        format!("- Follow-up signals: {}", assessment.follow_up_signal_count),
        format!("- Assessment: {}", assessment.summary),
        String::new(),
        "## Attention Reasons".to_string(),
    ];
    if assessment.attention_reasons.is_empty() {
        lines.push("- No explicit attention reason was derived; inspect the latest pool state before acting.".to_string());
    } else {
        lines.extend(
            assessment
                .attention_reasons
                .iter()
                .map(|reason| format!("- {}", reason)),
        );
    }
    if let Some(org_spec) = attention.org_spec_excerpt.as_deref() {
        lines.extend([
            String::new(),
            "## Project org_spec (convergence reference)".to_string(),
            "- Compare all phases, milestones and deliverables against StaffDeck task evidence, not just whether existing tasks are done.".to_string(),
            org_spec.to_string(),
        ]);
    }
    lines.extend([String::new(), "## Guidance".to_string()]);

    match assessment.decision {
        ProjectDecision::Continue => {
            lines.push(
                "The project snapshot still shows active coordination pressure. Inspect staff_task_list for this project and its org_spec before deciding whether Xiaoheng should act now or continue waiting."
                    .to_string(),
            );
            if assessment.active_todo_count == 0
                && (assessment.follow_up_signal_count > 0 || assessment.task_failed_count > 0)
            {
                lines.push(
                    "There is coordination pressure without a clear active owner. Treat this as an attention gap to investigate, not as permission to pick a specific next actor automatically."
                        .to_string(),
                );
            }
            lines.push(
                "Do not assume a fixed reviewer or handoff target. Use the current pool evidence and org_spec to decide the smallest effective coordination action."
                    .to_string(),
            );
        }
        ProjectDecision::SupervisorDecisionRequired => {
            lines.push(
                "Workers look locally finished; the task board alone is NOT proof the project converged under org_spec."
                    .to_string(),
            );
            lines.push(
                "Mandatory before HEARTBEAT_OK: call staff_project_list and staff_task_list(project_id=...). Compare every orgSpec requirement against the task evidence."
                    .to_string(),
            );
            lines.push(
                "If orgSpec still has uncovered work, use staff_list and staff_task_create for the next concrete step. Do not reply HEARTBEAT_OK while the specification is unfinished."
                    .to_string(),
            );
            lines.push(
                "If user input is required, explain the exact decision and evidence in the final response; the host will surface it in the project UI."
                    .to_string(),
            );
        }
        ProjectDecision::EscalateToHuman => {
            lines.push(
                "The project reached a state that should NOT be resolved by further autonomous retries or automatic routing."
                    .to_string(),
            );
            lines.push(
                "Treat this as a human-escalation state: inspect the failure evidence, summarize what became impossible or unsafe to decide automatically, and stop short of inventing a new worker plan unless the user explicitly approves it."
                    .to_string(),
            );
            lines.push(
                "Your role here is to surface the situation clearly for the user, not to silently convert an unrecoverable failure into a normal project continuation."
                    .to_string(),
            );
            lines.push(
                "Return a concise critical summary explaining why human judgment is required; the host will surface it to the user."
                    .to_string(),
            );
        }
        ProjectDecision::ReadyForCoordinatorReview => {
            lines.push(
                "The snapshot is compatible with Xiaoheng review, but HEARTBEAT_OK is forbidden as the only action when needs_review tasks exist."
                    .to_string(),
            );
            if assessment.integration_ready_count > 0 {
                lines.push(format!(
                    "{} branch(es) are done on the board but not merged. Review the reported result and update the corresponding StaffDeck task; do not claim completion without integration evidence.",
                    assessment.integration_ready_count
                ));
            }
            lines.push(
                "Mandatory closeout: call staff_task_list(project_id=...), inspect reported deliverables, then use staff_task_update or create a focused rework task."
                    .to_string(),
            );
            lines.push(
                "Do not say \"no change\", \"无需干预\", or HEARTBEAT_OK until you have taken a concrete closeout action or clearly explained why no autonomous action is safe."
                    .to_string(),
            );
            lines.push(
                "During heartbeat, do NOT archive the project automatically. Leave the pool active until the user explicitly asks to archive it."
                    .to_string(),
            );
            lines.push("If you discover unresolved work, keep the project open and coordinate the next step explicitly.".to_string());
        }
    }

    lines.push(String::new());
    lines.push(
        "Do not treat a quiet board as success. Read orgSpec via staff_project_list, compare staff_task_list evidence, then act or document why HEARTBEAT_OK is justified."
            .to_string(),
    );
    lines.join("\n")
}

pub fn collect_pool_attention(
    pool: &PoolSession,
    messages: &[PoolMessage],
    todos: &[KoiTodo],
    koi_ids: &[String],
    last_seen_message_id: i64,
) -> Option<PoolAttention> {
    let latest_message_id = messages
        .last()
        .map(|m| m.id)
        .unwrap_or(last_seen_message_id);
    let new_attention_messages: Vec<&PoolMessage> = messages
        .iter()
        .filter(|m| m.id > last_seen_message_id && is_attention_event(m, koi_ids))
        .collect();

    let assessment = assess_project_state(messages, todos, koi_ids);
    let has_historic_piscis_route = messages
        .iter()
        .any(|msg| contains_coordinator_mention(&msg.content));

    let has_state_attention = !assessment.attention_reasons.is_empty();
    if new_attention_messages.is_empty()
        && assessment.decision == ProjectDecision::EscalateToHuman
        && has_historic_piscis_route
    {
        return None;
    }
    // Do not suppress supervisor attention after a prior @piscis mention: the board
    // may be quiet while org_spec is still unfinished (e.g. only early milestones done).
    if new_attention_messages.is_empty()
        && assessment.decision == ProjectDecision::Continue
        && !has_state_attention
    {
        return None;
    }
    let mut lines = vec![
        format!("Pool: {} ({})", pool.name, pool.id),
        format!("Status: {}", pool.status),
        format!("Recent attention events: {}", new_attention_messages.len()),
        format!("Assessment: {}", assessment.summary),
    ];
    if let Some(project_dir) = pool.project_dir.as_deref() {
        lines.push(format!("Project dir: {}", project_dir));
    }
    lines.push("Recent pool events:".to_string());
    for msg in new_attention_messages.iter().rev().take(6).rev() {
        let event = msg.event_type.as_deref().unwrap_or(&msg.msg_type);
        lines.push(format!(
            "- #{} [{}] {}: {}",
            msg.id,
            event,
            msg.sender_id,
            preview_chars(&msg.content.replace('\n', " "), 240)
        ));
    }

    let org_spec_excerpt = {
        let trimmed = pool.org_spec.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(preview_chars(trimmed, 4000))
        }
    };

    Some(PoolAttention {
        pool_id: pool.id.clone(),
        pool_name: pool.name.clone(),
        latest_message_id,
        session_id: pool_coordinator_session_id(&pool.id),
        summary: lines.join("\n"),
        assessment,
        org_spec_excerpt,
    })
}

/// Build attention for an explicit `@!Piscis` mention without relying on the
/// periodic heartbeat cursor. Uses a `last_seen` one message before the
/// triggering mention so the mention is always included in the inbox summary.
pub fn build_forced_mention_attention(
    pool: &PoolSession,
    messages: &[PoolMessage],
    todos: &[KoiTodo],
    koi_ids: &[String],
) -> Option<PoolAttention> {
    let trigger = messages
        .iter()
        .rev()
        .find(|msg| contains_delegated_coordinator_mention(&msg.content))?;
    collect_pool_attention(pool, messages, todos, koi_ids, trigger.id.saturating_sub(1))
}
