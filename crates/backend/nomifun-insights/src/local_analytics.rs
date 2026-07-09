//! Local usage analytics (optimization 8).
//!
//! Aggregates data from conversations, companion skills, and companion
//! memories into a single response for the analytics dashboard UI.
//! The actual data collection is done by the caller (companion service,
//! which has access to both stores); this module defines the serializable
//! response structures and a builder function.

use serde::Serialize;

/// Summary of conversation activity.
#[derive(Debug, Serialize, Default)]
pub struct ConversationAnalytics {
    /// Total conversations (all time).
    pub total_conversations: u64,
    /// Conversations active in the last 7 days.
    pub active_conversations_7d: u64,
    /// Total messages sent (all time).
    pub total_messages: u64,
    /// Messages sent in the last 7 days.
    pub messages_7d: u64,
}

/// Summary of companion skill statistics.
#[derive(Debug, Serialize, Default)]
pub struct SkillAnalytics {
    /// Skills by status: (active, draft, archived).
    pub by_status: SkillStatusCounts,
    /// Skills by source: (mined, manual, imported).
    pub by_source: SkillSourceCounts,
    /// Top 5 most-used skills (name, usage_count).
    pub top_by_usage: Vec<SkillUsageEntry>,
    /// Average strength across active skills.
    pub avg_strength: f64,
    /// Average confidence across active skills.
    pub avg_confidence: f64,
}

#[derive(Debug, Serialize, Default)]
pub struct SkillStatusCounts {
    pub active: u64,
    pub draft: u64,
    pub archived: u64,
}

#[derive(Debug, Serialize, Default)]
pub struct SkillSourceCounts {
    pub mined: u64,
    pub manual: u64,
    pub imported: u64,
}

#[derive(Debug, Serialize)]
pub struct SkillUsageEntry {
    pub name: String,
    pub usage_count: i64,
    pub strength: f64,
}

/// Summary of companion memory statistics.
#[derive(Debug, Serialize, Default)]
pub struct MemoryAnalytics {
    /// Total active memories.
    pub total_active: u64,
    /// Memories by kind.
    pub by_kind: MemoryKindCounts,
    /// Average importance across active memories.
    pub avg_importance: f64,
    /// Average strength across active memories.
    pub avg_strength: f64,
    /// Pinned memories count.
    pub pinned: u64,
}

#[derive(Debug, Serialize, Default)]
pub struct MemoryKindCounts {
    pub profile: u64,
    pub preference: u64,
    pub knowledge: u64,
    pub episode: u64,
    pub task: u64,
    pub affective: u64,
}

/// Summary of learning run statistics.
#[derive(Debug, Serialize, Default)]
pub struct LearningAnalytics {
    /// Total learning runs.
    pub total_runs: u64,
    /// Runs in the last 7 days.
    pub runs_7d: u64,
    /// Total memories added across all runs.
    pub total_memories_added: u64,
    /// Total suggestions added across all runs.
    pub total_suggestions_added: u64,
}

/// The complete local analytics response.
#[derive(Debug, Serialize, Default)]
pub struct LocalAnalytics {
    pub conversations: ConversationAnalytics,
    pub skills: SkillAnalytics,
    pub memories: MemoryAnalytics,
    pub learning: LearningAnalytics,
    /// When this snapshot was generated (unix ms).
    pub generated_at: i64,
}
