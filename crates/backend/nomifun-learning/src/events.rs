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
        self.broadcast("learning.course-generation", payload);
    }

    /// Same stream shape for on-demand lesson content generation, tagged
    /// with the lesson so concurrent generations stay separable.
    pub fn emit_lesson_generation(&self, payload: &serde_json::Value) {
        self.broadcast("learning.lesson-generation", payload);
    }
}
