//! Real-time course/lesson generation events for the Learning page. Mirrors
//! `nomifun_knowledge::KnowledgeEventEmitter`: a thin best-effort wrapper
//! around the host `UserEventSink` — serialization or delivery failures are
//! logged and swallowed, never surfaced to the generation pipeline.
//!
//! One event name per stream (`learning.course-generation` /
//! `learning.lesson-generation`); the payload's `phase` field discriminates
//! the stages. WS delivery is fire-and-forget: frames are not replayed on
//! reconnect, so the frontend treats the HTTP response as the single source
//! of truth for the terminal state.

use std::sync::Arc;

use nomifun_api_types::WebSocketMessage;
use nomifun_realtime::UserEventSink;

/// 课程流事件名（x11 单源；前端据此订阅）。
pub const EVENT_COURSE_GENERATION: &str = "learning.course-generation";
/// 课时流事件名。
pub const EVENT_LESSON_GENERATION: &str = "learning.lesson-generation";

/// WS 进度帧 `phase` 取值单源（x11）：payload 仍是手拼 JSON，但字符串只许
/// 从这里引用；分组清单供 x12 互钉脚本与前端 phaseMeta 对齐。
pub mod phase {
    // 核心四态。
    pub const STARTED: &str = "started";
    pub const COMPLETED: &str = "completed";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";
    // 课程流。
    pub const PUBLISHING: &str = "publishing";
    pub const TRIAGE_STARTED: &str = "triage_started";
    pub const TRIAGE_COMPLETED: &str = "triage_completed";
    pub const SOURCE_PLAN_STARTED: &str = "source_plan_started";
    pub const SOURCE_PLAN_READY: &str = "source_plan_ready";
    pub const COVERAGE_READY: &str = "coverage_ready";
    pub const BASIS_DECLARED: &str = "basis_declared";
    pub const BASIS_REVOKED: &str = "basis_revoked";
    // 课时流。
    pub const RESUMED: &str = "resumed";
    pub const RETRY: &str = "retry";
    pub const GROUNDING: &str = "grounding";
    pub const DEGRADED: &str = "degraded";
    // 循环帧（AG 翻译层透传）。
    pub const ROUND: &str = "round";
    pub const AUDIT: &str = "audit";
    pub const GENERATE: &str = "generate";
    pub const REPAIR: &str = "repair";

    pub const CORE: &[&str] = &[STARTED, COMPLETED, FAILED, CANCELLED];
    pub const COURSE: &[&str] = &[
        PUBLISHING,
        TRIAGE_STARTED,
        TRIAGE_COMPLETED,
        SOURCE_PLAN_STARTED,
        SOURCE_PLAN_READY,
        COVERAGE_READY,
        BASIS_DECLARED,
        BASIS_REVOKED,
    ];
    pub const LESSON: &[&str] = &[RESUMED, RETRY, GROUNDING, DEGRADED];
    pub const LOOP: &[&str] = &[ROUND, AUDIT, GENERATE, REPAIR];
    /// 四组去重拼接（20 值）。
    pub const ALL: &[&str] = &[
        STARTED,
        COMPLETED,
        FAILED,
        CANCELLED,
        PUBLISHING,
        TRIAGE_STARTED,
        TRIAGE_COMPLETED,
        SOURCE_PLAN_STARTED,
        SOURCE_PLAN_READY,
        COVERAGE_READY,
        BASIS_DECLARED,
        BASIS_REVOKED,
        RESUMED,
        RETRY,
        GROUNDING,
        DEGRADED,
        ROUND,
        AUDIT,
        GENERATE,
        REPAIR,
    ];
}

#[derive(Clone)]
pub struct LearningEventEmitter {
    sink: Arc<dyn UserEventSink>,
    authoritative_user_id: Arc<str>,
}

impl LearningEventEmitter {
    pub fn new(sink: Arc<dyn UserEventSink>, authoritative_user_id: Arc<str>) -> Self {
        Self {
            sink,
            authoritative_user_id,
        }
    }

    fn broadcast<T: serde::Serialize>(&self, event_name: &str, payload: &T) {
        let value = match serde_json::to_value(payload) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(error = %error, event_name, "failed to serialize learning event");
                return;
            }
        };
        self.sink.send_to_user(
            &self.authoritative_user_id,
            WebSocketMessage::new(event_name, value),
        );
    }

    /// One progress frame of a course-outline generation. `payload` is the
    /// caller-built JSON with a `phase` field (`started`, `scope`, `round`,
    /// `audit`, `publishing`, `completed`, `failed`).
    pub fn emit_course_generation(&self, payload: &serde_json::Value) {
        self.broadcast(EVENT_COURSE_GENERATION, payload);
    }

    /// Same stream shape for on-demand lesson content generation, tagged
    /// with the lesson so concurrent generations stay separable.
    pub fn emit_lesson_generation(&self, payload: &serde_json::Value) {
        self.broadcast(EVENT_LESSON_GENERATION, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 导出机器可读 phase 清单（x12 互钉脚本消费）：每次 cargo test 重写
    /// `<workspace>/target/learning-phase-manifest.json`。
    #[test]
    fn export_phase_manifest() {
        let manifest = serde_json::json!({
            "events": {
                "course": EVENT_COURSE_GENERATION,
                "lesson": EVENT_LESSON_GENERATION,
            },
            "phases": {
                "core": phase::CORE,
                "course": phase::COURSE,
                "lesson": phase::LESSON,
                "loop": phase::LOOP,
                "all": phase::ALL,
            },
        });
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let target = workspace.join("target");
        std::fs::create_dir_all(&target).expect("create target dir");
        std::fs::write(
            target.join("learning-phase-manifest.json"),
            serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");

        // 清单自检：四组去重拼接 = ALL（20 值）。
        let mut unique = phase::ALL.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), phase::ALL.len(), "phase manifest has duplicates");
        assert_eq!(phase::ALL.len(), 20);
    }
}
