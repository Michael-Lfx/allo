//! Session observation for non-streaming auxiliary / side-model LLM calls.

use std::sync::Arc;

use nomi_agent_trace::{ObservationIds, ObservationScope};
use nomi_auxiliary::{AuxiliaryLlmObserver, AuxiliaryObservationContext};
use nomi_types::message::Message;

use crate::observation::{
    begin_chat_llm, finish_chat_llm, messages_to_llm_request, ObservationSession,
};

pub const CALL_KIND_CONVERSATION_TITLE: &str = "conversation_title";
pub const CALL_KIND_POI_EXTRACT: &str = "poi_extract";
pub const CALL_KIND_POI_STARTER: &str = "poi_starter";
pub const CALL_KIND_RESOLUTION: &str = "resolution";

/// Binds one conversation on a shared recorder (auxiliary calls share the main JSONL).
pub fn conversation_observation_session(
    recorder: Arc<nomi_agent_trace::ObservationRecorder>,
    conversation_id: &str,
) -> Arc<ObservationSession> {
    let session = ObservationSession::new(recorder);
    session.bind_ids(ObservationIds {
        conversation_id: Some(conversation_id.to_string()),
        session_kind: Some("session_dialogue".to_string()),
        ..Default::default()
    });
    session
}

pub struct SessionAuxiliaryObserver {
    session: Arc<ObservationSession>,
    scope: ObservationScope,
}

impl SessionAuxiliaryObserver {
    pub fn new(session: Arc<ObservationSession>, scope: ObservationScope) -> Self {
        Self { session, scope }
    }

    pub fn session_auxiliary(session: Arc<ObservationSession>) -> Arc<dyn AuxiliaryLlmObserver> {
        Arc::new(Self::new(session, ObservationScope::SessionAuxiliary))
    }
}

impl AuxiliaryLlmObserver for SessionAuxiliaryObserver {
    fn on_llm_request(
        &self,
        ctx: &AuxiliaryObservationContext,
        call_kind: &str,
        observation_scope: &str,
        model: &str,
        messages: &[Message],
    ) -> String {
        if let Some(id) = ctx
            .conversation_id
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            self.session.bind_ids(ObservationIds {
                conversation_id: Some(id.to_string()),
                session_kind: ctx
                    .session_kind
                    .clone()
                    .or_else(|| Some("session_dialogue".to_string())),
                ..Default::default()
            });
        }
        let scope = parse_scope(observation_scope);
        let request = messages_to_llm_request(model, messages);
        begin_chat_llm(&self.session, call_kind, scope, &request)
    }

    fn on_llm_response(
        &self,
        model_call_id: &str,
        call_kind: &str,
        text: &str,
        elapsed_ms: u64,
        error: Option<&str>,
    ) {
        finish_chat_llm(
            &self.session,
            model_call_id,
            call_kind,
            self.scope,
            text,
            elapsed_ms,
            error,
        );
    }
}

fn parse_scope(raw: &str) -> ObservationScope {
    match raw {
        "session_workflow" => ObservationScope::SessionWorkflow,
        "session_auxiliary" => ObservationScope::SessionAuxiliary,
        _ => ObservationScope::ProcessDiagnostic,
    }
}

#[cfg(test)]
mod tests {
    use nomi_agent_trace::{EVENT_LLM_REQUEST, EVENT_LLM_RESPONSE, ObservationRecorder};
    use nomi_auxiliary::{AuxiliaryLlmObserver, AuxiliaryObservationContext};
    use nomi_types::message::Role;

    use super::*;

    #[test]
    fn session_auxiliary_observer_writes_call_kind_to_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let recorder = ObservationRecorder::isolated(dir.path());
        recorder.set_enabled(true);
        let session = conversation_observation_session(recorder.clone(), "conv-aux-1");
        let observer: Arc<dyn AuxiliaryLlmObserver> =
            SessionAuxiliaryObserver::session_auxiliary(session);
        let ctx = AuxiliaryObservationContext {
            conversation_id: Some("conv-aux-1".into()),
            session_kind: Some("session_dialogue".into()),
        };
        let messages = vec![nomi_auxiliary::text_message(Role::User, "extract topics")];
        let call_id = observer.on_llm_request(
            &ctx,
            CALL_KIND_POI_EXTRACT,
            "session_auxiliary",
            "side-model",
            &messages,
        );
        observer.on_llm_response(&call_id, CALL_KIND_POI_EXTRACT, "[]", 12, None);

        let events = recorder.read_events(Some("conv-aux-1")).expect("read");
        let request = events
            .iter()
            .find(|e| e.event_type == EVENT_LLM_REQUEST)
            .expect("llm/request");
        assert_eq!(request.payload["call_kind"], CALL_KIND_POI_EXTRACT);
        assert_eq!(
            request.payload["observation_scope"].as_str(),
            Some("session_auxiliary")
        );
        let response = events
            .iter()
            .find(|e| e.event_type == EVENT_LLM_RESPONSE)
            .expect("llm/response");
        assert_eq!(response.payload["call_kind"], CALL_KIND_POI_EXTRACT);
        assert_eq!(response.payload["text"], "[]");
        let encoded = request.payload.to_string();
        assert!(!encoded.contains("…(truncated)"));
    }
}
