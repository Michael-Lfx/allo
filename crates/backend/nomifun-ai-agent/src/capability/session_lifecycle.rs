//! Conversation session lifecycle hooks (pre/post turn, session end).
//!
//! [`SessionLifecycleCoordinator`] is wired by `nomifun-conversation` on
//! message send and session teardown; POI prefetch for ACP agents uses the
//! separate [`crate::capability::prompt_pipeline::PreSendHook`] path.

use std::path::PathBuf;
use std::sync::Arc;

use nomi_config::InterestConfig;
use nomi_insights_core::{spawn_session_end_pipeline, touch_active_session};
use nomifun_insights::InsightsService;
use nomifun_poi::PoiService;

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

#[async_trait::async_trait]
pub trait SessionEndHook: Send + Sync {
    async fn on_session_end(&self, ctx: &SessionEndContext);
}

pub struct SessionLifecycleCoordinator {
    pre_turn: Vec<Arc<dyn PreTurnHook>>,
    post_turn: Vec<Arc<dyn PostTurnHook>>,
    session_end: Vec<Arc<dyn SessionEndHook>>,
    insights_data_dir: Option<PathBuf>,
}

pub struct SessionLifecycleCoordinatorBuilder {
    pre_turn: Vec<Arc<dyn PreTurnHook>>,
    post_turn: Vec<Arc<dyn PostTurnHook>>,
    session_end: Vec<Arc<dyn SessionEndHook>>,
    insights_data_dir: Option<PathBuf>,
}

impl SessionLifecycleCoordinator {
    pub fn builder() -> SessionLifecycleCoordinatorBuilder {
        SessionLifecycleCoordinatorBuilder {
            pre_turn: Vec::new(),
            post_turn: Vec::new(),
            session_end: Vec::new(),
            insights_data_dir: None,
        }
    }

    /// Default wiring for desktop conversation sessions using app services.
    pub fn from_poi_and_insights(
        poi_service: Arc<PoiService>,
        insights_service: Arc<InsightsService>,
    ) -> Self {
        Self::builder()
            .insights_data_dir(insights_service.data_dir().to_path_buf())
            .session_end(Arc::new(WorkSessionEndHook::new(
                poi_service,
                insights_service,
            )))
            .build()
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

    pub async fn run_session_end(&self, ctx: &SessionEndContext) {
        for hook in &self.session_end {
            hook.on_session_end(ctx).await;
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

    pub fn session_end(mut self, hook: Arc<dyn SessionEndHook>) -> Self {
        self.session_end.push(hook);
        self
    }

    pub fn insights_data_dir(mut self, path: PathBuf) -> Self {
        self.insights_data_dir = Some(path);
        self
    }

    pub fn build(self) -> SessionLifecycleCoordinator {
        SessionLifecycleCoordinator {
            pre_turn: self.pre_turn,
            post_turn: self.post_turn,
            session_end: self.session_end,
            insights_data_dir: self.insights_data_dir,
        }
    }
}

/// Session-end hook: POI ingest (interest.db) + insights work packages.
pub struct WorkSessionEndHook {
    poi_service: Arc<PoiService>,
    insights_service: Arc<InsightsService>,
}

impl WorkSessionEndHook {
    pub fn new(poi_service: Arc<PoiService>, insights_service: Arc<InsightsService>) -> Self {
        Self {
            poi_service,
            insights_service,
        }
    }
}

#[async_trait::async_trait]
impl SessionEndHook for WorkSessionEndHook {
    async fn on_session_end(&self, ctx: &SessionEndContext) {
        let interest_cfg = self.poi_service.interest_config();
        let insights_cfg = self.insights_service.contribution_config().await;
        if !interest_cfg.enabled && !insights_cfg.enabled {
            return;
        }

        let poi_data_dir = self.poi_service.data_dir().to_path_buf();
        let insights_data_dir = self.insights_service.data_dir().to_path_buf();
        let session_id = ctx.session_id.clone();
        let messages = ctx.messages.clone();

        if interest_cfg.enabled {
            let mut insights_off = insights_cfg.clone();
            insights_off.enabled = false;
            spawn_session_end_pipeline(
                poi_data_dir,
                interest_cfg,
                insights_off,
                session_id.clone(),
                messages.clone(),
                Vec::new(),
                None,
            );
        }

        if insights_cfg.enabled {
            let mut interest_off = InterestConfig::default();
            interest_off.enabled = false;
            spawn_session_end_pipeline(
                insights_data_dir,
                interest_off,
                insights_cfg,
                session_id,
                messages,
                Vec::new(),
                None,
            );
        }
    }
}
