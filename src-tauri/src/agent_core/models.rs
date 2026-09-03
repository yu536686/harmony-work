use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KoiTodo {
    pub id: String,
    pub owner_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub assigned_by: String,
    pub pool_session_id: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub depends_on: Option<String>,
    pub blocked_reason: Option<String>,
    /// Git branch created for this todo's worktree, e.g. `koi/coder-a1b2c3d4`.
    #[serde(default)]
    pub git_branch: Option<String>,
    /// Integration lifecycle: none | ready | merged | conflict.
    #[serde(default)]
    pub integration_status: Option<String>,
    pub result_message_id: Option<i64>,
    pub source_type: String,
    #[serde(default)]
    pub task_timeout_secs: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSession {
    pub id: String,
    pub name: String,
    pub org_spec: String,
    pub status: String,
    pub project_dir: Option<String>,
    #[serde(default)]
    pub task_timeout_secs: u32,
    /// `binding_key` of the IM conversation that originally requested
    /// this pool, when applicable. Allows pool-level events (heartbeat
    /// alerts, decision requests) to fan out to the originating IM
    /// session in addition to the desktop UI. `None` for pools created
    /// from the desktop chat or via CLI/headless paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_im_binding_key: Option<String>,
    /// Ids of the Koi that are members of this project pool. Only members
    /// appear as participants and can be assigned work. Populated by the
    /// store on list/get; defaults to empty when not hydrated.
    #[serde(default)]
    pub member_koi_ids: Vec<String>,
    /// Owning swarm/workflow team slug (AgentZ `team.json` id). Used to
    /// isolate pools so same-named teams with different ids do not share state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// When set, this pool backs a single workflow run transcript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<String>,
    pub last_active_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMessage {
    pub id: i64,
    pub pool_session_id: String,
    pub sender_id: String,
    pub content: String,
    pub msg_type: String,
    pub metadata: String,
    pub todo_id: Option<String>,
    pub reply_to_message_id: Option<i64>,
    pub event_type: Option<String>,
    pub created_at: DateTime<Utc>,
}

// -- Koi definitions -------------------------------------------------
// Kept in `piscis-core::models` so that both the kernel's database layer
// and the desktop-side koi runtime can share the same data shapes
// without creating a kernel → desktop dependency.

#[derive(Debug, Clone, Copy)]
pub struct StarterKoiSpec {
    pub name: &'static str,
    pub role: &'static str,
    pub icon: &'static str,
    pub color: &'static str,
    pub system_prompt: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KoiDefinition {
    pub id: String,
    pub name: String,
    pub role: String,
    pub icon: String,
    pub color: String,
    pub system_prompt: String,
    pub description: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub llm_provider_id: Option<String>,
    #[serde(default)]
    pub max_iterations: u32,
    #[serde(default)]
    pub task_timeout_secs: u32,
}

/// Compact, user-editable capability view derived from a Koi's existing
/// role and description fields. Keeping this derived avoids a migration and
/// makes edits made in the People UI effective immediately.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KoiCapabilityProfile {
    pub koi_id: String,
    pub koi_name: String,
    pub role: String,
    pub status: String,
    pub capabilities: Vec<String>,
}

/// A deterministic candidate ranking for a task. The coordinator uses this
/// as evidence when it decides whether and how to delegate; it is never an
/// implicit task assignment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KoiCapabilityCandidate {
    pub koi_id: String,
    pub koi_name: String,
    pub role: String,
    pub status: String,
    pub score: u32,
    pub matching_capabilities: Vec<String>,
    pub capabilities: Vec<String>,
}

impl KoiDefinition {
    pub fn capability_profile(&self) -> KoiCapabilityProfile {
        let mut capabilities = Vec::new();
        push_capability(&mut capabilities, &self.role);
        for part in self
            .description
            .split([',', ';', '\n', '\r', '，', '、', '；', '|', '/'])
        {
            push_capability(&mut capabilities, part);
        }

        KoiCapabilityProfile {
            koi_id: self.id.clone(),
            koi_name: self.name.clone(),
            role: self.role.clone(),
            status: self.status.clone(),
            capabilities,
        }
    }
}

/// Rank online Koi by explicit capability phrases that occur in a task.
/// Ties are stable by name/id so the same staff profile produces the same
/// recommendation across runs.
pub fn rank_kois_for_task(
    kois: &[KoiDefinition],
    task: &str,
    limit: usize,
) -> Vec<KoiCapabilityCandidate> {
    let normalized_task = normalize_capability_text(task);
    let mut candidates: Vec<KoiCapabilityCandidate> = kois
        .iter()
        .filter(|koi| !koi.status.eq_ignore_ascii_case("offline"))
        .map(|koi| {
            let profile = koi.capability_profile();
            let matching_capabilities: Vec<String> = profile
                .capabilities
                .iter()
                .filter(|capability| {
                    let normalized = normalize_capability_text(capability);
                    normalized.chars().count() >= 2 && normalized_task.contains(&normalized)
                })
                .cloned()
                .collect();
            let availability_bonus = u32::from(koi.status.eq_ignore_ascii_case("idle"));
            let score = matching_capabilities.len() as u32 * 100 + availability_bonus;

            KoiCapabilityCandidate {
                koi_id: profile.koi_id,
                koi_name: profile.koi_name,
                role: profile.role,
                status: profile.status,
                score,
                matching_capabilities,
                capabilities: profile.capabilities,
            }
        })
        .collect();

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.koi_name.cmp(&right.koi_name))
            .then_with(|| left.koi_id.cmp(&right.koi_id))
    });
    candidates.truncate(limit.max(1));
    candidates
}

fn push_capability(capabilities: &mut Vec<String>, raw: &str) {
    let trimmed = raw.trim();
    let without_label = [
        "能力：",
        "能力:",
        "技能：",
        "技能:",
        "capabilities:",
        "capability:",
    ]
    .iter()
    .find_map(|label| trimmed.strip_prefix(label))
    .unwrap_or(trimmed)
    .trim();
    if without_label.is_empty()
        || capabilities
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(without_label))
    {
        return;
    }
    capabilities.push(without_label.to_string());
}

fn normalize_capability_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub const KOI_COLORS: &[(&str, &str)] = &[
    ("#7c6af7", "Violet"),
    ("#4ecdc4", "Teal"),
    ("#45b7d1", "Sky"),
    ("#f7b731", "Gold"),
    ("#fc5c65", "Coral"),
    ("#26de81", "Emerald"),
    ("#a55eea", "Purple"),
    ("#fd9644", "Orange"),
    ("#778ca3", "Steel"),
    ("#eb3b5a", "Rose"),
    ("#20bf6b", "Green"),
    ("#2d98da", "Blue"),
];

pub const KOI_ICONS: &[&str] = &[
    "\u{1F419}",
    "\u{1F988}",
    "\u{1F42C}",
    "\u{1F991}",
    "\u{1F433}",
    "\u{1F41F}",
    "\u{1F990}",
    "\u{1F980}",
    "\u{1F916}",
    "\u{1F4CA}",
    "\u{1F3A8}",
    "\u{1F4BB}",
    "\u{1F52C}",
    "\u{1F4DD}",
    "\u{1F6E1}\u{FE0F}",
    "\u{1F310}",
    "\u{1F9E0}",
    "\u{26A1}",
    "\u{1F527}",
    "\u{1F4C1}",
    "\u{1F3AF}",
    "\u{1F3D7}\u{FE0F}",
    "\u{1F50D}",
    "\u{1F4E1}",
];

pub const STARTER_KOI_SPECS: &[StarterKoiSpec] = &[
    StarterKoiSpec {
        name: "Architect",
        role: "\u{67B6}\u{6784}\u{5E08}",
        icon: "\u{1F3D7}\u{FE0F}",
        color: "#7c6af7",
        system_prompt:
            "You are a software architect. Your job is to design clear, practical technical specifications. \
             Be concise and structured. Output designs as numbered plans with explicit trade-offs, interfaces, and handoff points.",
        description: "Architecture, system design, technical specification",
    },
    StarterKoiSpec {
        name: "Coder",
        role: "\u{7A0B}\u{5E8F}\u{5458}",
        icon: "\u{1F4BB}",
        color: "#45b7d1",
        system_prompt:
            "You are a software developer. Given a specification, write clean, working code. \
             Be practical, prioritize correctness, and explain important implementation choices briefly.",
        description: "Implementation, coding, development",
    },
    StarterKoiSpec {
        name: "Reviewer",
        role: "\u{4EE3}\u{7801}\u{5BA1}\u{67E5}\u{5458}",
        icon: "\u{1F50D}",
        color: "#26de81",
        system_prompt:
            "You are a code reviewer. Review designs and code critically but constructively. \
             Point out concrete risks, missing tests, regressions, and the smallest safe improvements.",
        description: "Code review, quality assurance, feedback",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn koi(id: &str, role: &str, description: &str, status: &str) -> KoiDefinition {
        KoiDefinition {
            id: id.into(),
            name: id.into(),
            role: role.into(),
            icon: String::new(),
            color: String::new(),
            system_prompt: String::new(),
            description: description.into(),
            status: status.into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            llm_provider_id: None,
            max_iterations: 0,
            task_timeout_secs: 0,
        }
    }

    #[test]
    fn ranks_available_koi_by_declared_capabilities() {
        let product = koi(
            "product",
            "产品经理",
            "能力：需求分析、产品策略、PPT",
            "idle",
        );
        let writer = koi("writer", "内容经理", "能力：内容策划、文案", "idle");
        let offline = koi("offline", "设计师", "能力：PPT、视觉设计", "offline");

        let candidates =
            rank_kois_for_task(&[writer, offline, product], "请制作产品策略介绍 PPT", 3);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].koi_id, "product");
        assert_eq!(candidates[0].matching_capabilities, vec!["产品策略", "PPT"]);
        assert_eq!(candidates[1].koi_id, "writer");
    }
}
