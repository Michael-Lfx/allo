//! Conversation session lifecycle hooks (pre/post turn, session end).
//!
//! [`SessionLifecycleCoordinator`] is wired by `nomifun-conversation` on
//! message send and session teardown; POI prefetch for ACP agents uses the
//! separate [`crate::capability::prompt_pipeline::PreSendHook`] path.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_insights_core::touch_active_session;
use nomifun_insights::InsightsService;
use nomifun_poi::PoiService;

use super::proactive_extraction::{MessageLoader, ProactiveSessionExtractor};
use crate::auxiliary_provider::AuxiliaryClientFactory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason {
    Deleted,
    Reset,
    ClearMessages,
    NewSession,
}

pub struct TurnContext<'a> {
    pub conversation_id: &'a str,
    pub session_id: &'a str,
    pub user_prompt: &'a str,
    /// Whether this turn was initiated by a human (vs cron/autowork/idmm).
    /// Used by `PostTurnReviewHook` to skip non-human turns (optimization 2).
    pub origin_is_human: bool,
}

pub struct SessionEndContext {
    pub conversation_id: String,
    pub session_id: String,
    pub messages: Vec<serde_json::Value>,
    pub reason: SessionEndReason,
}

#[async_trait::async_trait]
pub trait PreTurnHook: Send + Sync {
    async fn on_pre_turn(&self, ctx: &TurnContext<'_>, prompt: String) -> String;
}

#[async_trait::async_trait]
pub trait PostTurnHook: Send + Sync {
    async fn on_post_turn(&self, ctx: &TurnContext<'_>, reply: &str) -> String;
}

/// Per-turn background review hook (optimization 2). Fired after each human-origin
/// turn to asynchronously evaluate whether memories or skills should be updated —
/// a lighter, more timely counterpart to the session-end distillation pipeline.
/// Implementations MUST be fire-and-forget (never block the conversation loop).
#[async_trait::async_trait]
pub trait PostTurnReviewHook: Send + Sync {
    async fn on_post_turn_review(&self, ctx: &TurnContext<'_>, reply: &str, messages: &[serde_json::Value]);
}

#[async_trait::async_trait]
pub trait SessionEndHook: Send + Sync {
    async fn on_session_end(&self, ctx: &SessionEndContext);
}

pub struct SessionLifecycleCoordinator {
    pre_turn: Vec<Arc<dyn PreTurnHook>>,
    post_turn: Vec<Arc<dyn PostTurnHook>>,
    post_turn_review: Vec<Arc<dyn PostTurnReviewHook>>,
    session_end: Vec<Arc<dyn SessionEndHook>>,
    insights_data_dir: Option<PathBuf>,
    extractor: Option<Arc<ProactiveSessionExtractor>>,
}

pub struct SessionLifecycleCoordinatorBuilder {
    pre_turn: Vec<Arc<dyn PreTurnHook>>,
    post_turn: Vec<Arc<dyn PostTurnHook>>,
    post_turn_review: Vec<Arc<dyn PostTurnReviewHook>>,
    session_end: Vec<Arc<dyn SessionEndHook>>,
    insights_data_dir: Option<PathBuf>,
    extractor: Option<Arc<ProactiveSessionExtractor>>,
}

impl SessionLifecycleCoordinator {
    pub fn builder() -> SessionLifecycleCoordinatorBuilder {
        SessionLifecycleCoordinatorBuilder {
            pre_turn: Vec::new(),
            post_turn: Vec::new(),
            post_turn_review: Vec::new(),
            session_end: Vec::new(),
            insights_data_dir: None,
            extractor: None,
        }
    }

    /// Default wiring for desktop conversation sessions using app services.
    pub fn from_poi_and_insights(
        poi_service: Arc<PoiService>,
        insights_service: Arc<InsightsService>,
        auxiliary_factory: Option<Arc<AuxiliaryClientFactory>>,
        message_loader: MessageLoader,
    ) -> Self {
        let extractor = Arc::new(
            ProactiveSessionExtractor::new(
                poi_service.clone(),
                insights_service.clone(),
                auxiliary_factory,
            )
            .with_message_loader(message_loader),
        );
        Self::builder()
            .insights_data_dir(insights_service.data_dir().to_path_buf())
            .extractor(extractor.clone())
            .session_end(Arc::new(WorkSessionEndHook::new(extractor)))
            .build()
    }

    pub fn proactive_extractor(&self) -> Option<Arc<ProactiveSessionExtractor>> {
        self.extractor.clone()
    }

    pub async fn run_pre_turn(&self, ctx: &TurnContext<'_>, prompt: String) -> String {
        let mut current = prompt;
        for hook in &self.pre_turn {
            current = hook.on_pre_turn(ctx, current).await;
        }
        current
    }

    pub async fn run_post_turn(&self, ctx: &TurnContext<'_>, reply: &str) -> String {
        let mut current = reply.to_owned();
        for hook in &self.post_turn {
            current = hook.on_post_turn(ctx, &current).await;
        }
        current
    }

    /// Fire all post-turn review hooks (optimization 2). Each hook is spawned
    /// asynchronously — this method returns immediately and never blocks the
    /// conversation loop. Only called for human-origin turns.
    pub fn run_post_turn_review(&self, ctx: &TurnContext<'_>, reply: &str, messages: &[serde_json::Value]) {
        if !ctx.origin_is_human {
            return;
        }
        for hook in &self.post_turn_review {
            let ctx_conv = ctx.conversation_id.to_string();
            let ctx_session = ctx.session_id.to_string();
            let ctx_prompt = ctx.user_prompt.to_string();
            let reply = reply.to_string();
            let messages = messages.to_vec();
            let hook = hook.clone();
            tokio::spawn(async move {
                let review_ctx = TurnContext {
                    conversation_id: &ctx_conv,
                    session_id: &ctx_session,
                    user_prompt: &ctx_prompt,
                    origin_is_human: true,
                };
                hook.on_post_turn_review(&review_ctx, &reply, &messages).await;
            });
        }
    }

    pub async fn run_session_end(&self, ctx: &SessionEndContext) {
        for hook in &self.session_end {
            hook.on_session_end(ctx).await;
        }
    }

    /// Touch active session + record user message for threshold-based extraction.
    pub async fn on_user_message(
        &self,
        session_id: &str,
        user_text: &str,
        message_count: usize,
        session_llm_model: Option<&str>,
    ) {
        self.touch_session(session_id);
        if let Some(extractor) = &self.extractor {
            extractor
                .on_user_message(session_id, user_text, message_count, session_llm_model)
                .await;
        }
    }

    pub fn touch_session(&self, session_id: &str) {
        let Some(dir) = self.insights_data_dir.as_ref() else {
            return;
        };
        touch_active_session(dir, session_id);
    }
}

impl SessionLifecycleCoordinatorBuilder {
    pub fn pre_turn(mut self, hook: Arc<dyn PreTurnHook>) -> Self {
        self.pre_turn.push(hook);
        self
    }

    pub fn post_turn(mut self, hook: Arc<dyn PostTurnHook>) -> Self {
        self.post_turn.push(hook);
        self
    }

    pub fn post_turn_review(mut self, hook: Arc<dyn PostTurnReviewHook>) -> Self {
        self.post_turn_review.push(hook);
        self
    }

    pub fn session_end(mut self, hook: Arc<dyn SessionEndHook>) -> Self {
        self.session_end.push(hook);
        self
    }

    pub fn insights_data_dir(mut self, path: PathBuf) -> Self {
        self.insights_data_dir = Some(path);
        self
    }

    pub fn extractor(mut self, extractor: Arc<ProactiveSessionExtractor>) -> Self {
        self.extractor = Some(extractor);
        self
    }

    pub fn build(self) -> SessionLifecycleCoordinator {
        SessionLifecycleCoordinator {
            pre_turn: self.pre_turn,
            post_turn: self.post_turn,
            post_turn_review: self.post_turn_review,
            session_end: self.session_end,
            insights_data_dir: self.insights_data_dir,
            extractor: self.extractor,
        }
    }
}

/// Session-end hook: final POI / insights flush via the proactive extractor.
pub struct WorkSessionEndHook {
    extractor: Arc<ProactiveSessionExtractor>,
}

impl WorkSessionEndHook {
    pub fn new(extractor: Arc<ProactiveSessionExtractor>) -> Self {
        Self { extractor }
    }
}

#[async_trait::async_trait]
impl SessionEndHook for WorkSessionEndHook {
    async fn on_session_end(&self, ctx: &SessionEndContext) {
        self.extractor
            .flush_on_session_end(&ctx.session_id, ctx.messages.clone())
            .await;
    }
}
