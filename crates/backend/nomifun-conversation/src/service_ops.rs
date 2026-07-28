//! Agent-session operations on ConversationService.
//!
//! These forward to the active AgentRuntimeHandle (via `self.runtime_handle(id)`) for
//! mode/model/usage/slash-commands/side-question/openclaw-runtime queries,
//! plus workspace browsing that needs the conversations.extra.workspace
//! field.
//!
//! Kept in a separate file from service.rs to avoid pushing that file
//! over 2000 lines.

use nomifun_api_types::{
    AgentModeResponse, GetModelInfoResponse, GoalActionRequest, GoalStatusResponse, SetModeRequest,
    SetModelRequest, SideQuestionRequest, SideQuestionResponse, SlashCommandItem,
    WorkspaceBrowseQuery, WorkspaceEntry,
};
use nomifun_common::AppError;
use nomifun_file::list_workspace_level;
use nomifun_db::models::ConversationRow;

use crate::service::ConversationService;

impl ConversationService {
    async fn require_owned_conversation(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<ConversationRow, AppError> {
        self.conversation_repo()
            .get(conversation_id)
            .await?
            .filter(|row| row.user_id == user_id)
            .ok_or_else(|| {
                AppError::NotFound(format!("Conversation '{conversation_id}' not found"))
            })
    }

    // ── Mode ────────────────────────────────────────────────────────

    pub async fn get_mode(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<AgentModeResponse, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        match self.optional_runtime_handle(conversation_id) {
            Some(runtime) => runtime.get_mode().await,
            None => Ok(AgentModeResponse {
                mode: "default".into(),
                initialized: false,
            }),
        }
    }

    pub async fn set_mode(
        &self,
        user_id: &str,
        conversation_id: &str,
        req: SetModeRequest,
    ) -> Result<(), AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        if req.mode.trim().is_empty() {
            return Err(AppError::BadRequest("mode must not be empty".into()));
        }
        self.runtime_handle(conversation_id)?.set_mode(&req.mode).await
    }

    // ── Model ───────────────────────────────────────────────────────

    pub async fn get_model(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<GetModelInfoResponse, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        match self.optional_runtime_handle(conversation_id) {
            Some(runtime) => runtime.get_model().await,
            None => Ok(GetModelInfoResponse { model_info: None }),
        }
    }

    pub async fn set_model(
        &self,
        user_id: &str,
        conversation_id: &str,
        req: SetModelRequest,
    ) -> Result<(), AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        if req.model_id.trim().is_empty() {
            return Err(AppError::BadRequest("model_id must not be empty".into()));
        }
        let runtime = match self.runtime_handle(conversation_id) {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::warn!(
                    conversation_id,
                    model_id = %req.model_id,
                    error_code = err.error_code(),
                    "Set model skipped because active Agent runtime is unavailable"
                );
                return Err(err);
            }
        };
        runtime.set_model(&req.model_id).await
    }

    // ── Usage / Slash commands ──────────────────────────────────────

    pub async fn get_usage(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<serde_json::Value>, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        match self.optional_runtime_handle(conversation_id) {
            Some(runtime) => runtime.get_usage().await,
            None => Ok(None),
        }
    }

    pub async fn get_slash_commands(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<SlashCommandItem>, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        match self.optional_runtime_handle(conversation_id) {
            Some(runtime) => runtime.get_slash_commands().await,
            None => Ok(Vec::new()),
        }
    }

    // ── Side question ───────────────────────────────────────────────

    pub async fn handle_side_question(
        &self,
        user_id: &str,
        conversation_id: &str,
        req: SideQuestionRequest,
    ) -> Result<SideQuestionResponse, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        // `AgentRuntimeHandle::handle_side_question` already validates that the
        // question is non-empty; no need to duplicate the check here.
        self.runtime_handle(conversation_id)?.handle_side_question(req).await
    }

    // ── OpenClaw runtime diagnostics ────────────────────────────────

    pub async fn get_openclaw_runtime(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<serde_json::Value, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        self.runtime_handle(conversation_id)?.get_openclaw_runtime().await
    }

    // ── Goal (auto-continuation) ────────────────────────────────────

    /// Apply a `/goal` action. A live agent runtime handles it in real time
    /// (and mirrors the result to the DB itself); without a runtime the
    /// action operates straight on the persisted snapshot, so the next
    /// session build restore-injects the outcome.
    pub async fn goal_action(
        &self,
        user_id: &str,
        conversation_id: &str,
        req: GoalActionRequest,
    ) -> Result<GoalStatusResponse, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        match self.optional_runtime_handle(conversation_id) {
            Some(runtime) => runtime.goal_action(req).await,
            None => self.goal_action_on_db(conversation_id, req).await,
        }
    }

    /// No-runtime fallback: mutate the persisted goal row via the shared
    /// `goal_bridge` helpers so the transition guards (terminal states never
    /// flip, resume resets counters) are the engine's own.
    async fn goal_action_on_db(
        &self,
        conversation_id: &str,
        req: GoalActionRequest,
    ) -> Result<GoalStatusResponse, AppError> {
        use nomifun_ai_agent::goal_bridge;

        let Some(repo) = self.goal_repo_slot() else {
            return Err(AppError::BadRequest(
                "The agent is not running and goal persistence is not enabled".into(),
            ));
        };
        match req.action.as_str() {
            "set" => {
                let objective = req
                    .objective
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| AppError::BadRequest("objective must not be empty".into()))?;
                // Same default/clamp as the live manager path.
                let max_turns = req.max_turns.unwrap_or(8).clamp(1, 100) as usize;
                let params =
                    goal_bridge::fresh_goal_upsert(conversation_id, objective.to_owned(), max_turns);
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            "pause" => {
                let row = self.require_goal_row(&*repo, conversation_id).await?;
                let params = goal_bridge::pause_persisted_row(
                    &row,
                    req.reason.as_deref().unwrap_or("user-paused"),
                );
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            "resume" => {
                let row = self.require_goal_row(&*repo, conversation_id).await?;
                let params = goal_bridge::resume_persisted_row(&row);
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            "clear" => {
                // Delete (not mark cleared): a cleared goal must never be
                // restore-injected into a future session build.
                repo.clear(conversation_id).await?;
                Ok(GoalStatusResponse::default())
            }
            "status" | "list_subgoals" => Ok(repo
                .load_by_session(conversation_id)
                .await?
                .map(|row| goal_bridge::goal_row_to_response(&row))
                .unwrap_or_default()),
            "add_subgoal" => {
                let text = req
                    .subgoal
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| AppError::BadRequest("subgoal must not be empty".into()))?;
                let row = self.require_goal_row(&*repo, conversation_id).await?;
                let params = goal_bridge::add_subgoal_row(&row, text);
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            "remove_subgoal" => {
                let index = req
                    .index_1based
                    .ok_or_else(|| AppError::BadRequest("index_1based is required".into()))?
                    as usize;
                let row = self.require_goal_row(&*repo, conversation_id).await?;
                let params = goal_bridge::remove_subgoal_row(&row, index).ok_or_else(|| {
                    AppError::BadRequest(format!("Subgoal index {index} is out of range"))
                })?;
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            "clear_subgoals" => {
                let row = self.require_goal_row(&*repo, conversation_id).await?;
                let params = goal_bridge::clear_subgoals_row(&row);
                let row = repo.upsert(&params).await?;
                Ok(goal_bridge::goal_row_to_response(&row))
            }
            other => Err(AppError::BadRequest(format!("Unknown goal action '{other}'"))),
        }
    }

    async fn require_goal_row(
        &self,
        repo: &dyn nomifun_db::IGoalRepository,
        conversation_id: &str,
    ) -> Result<nomifun_db::GoalRow, AppError> {
        repo.load_by_session(conversation_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("No goal is set for this conversation".into()))
    }

    /// Read the goal snapshot. Never errors on "no goal": a live Nomi
    /// runtime answers in real time; agent types without a goal runtime and
    /// non-running conversations fall back to the persisted row; no row (or
    /// persistence unwired) degrades to `active: false`.
    pub async fn get_goal_status(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<GoalStatusResponse, AppError> {
        self.require_owned_conversation(user_id, conversation_id)
            .await?;
        if let Some(runtime) = self.optional_runtime_handle(conversation_id) {
            let status_probe = GoalActionRequest {
                action: "status".into(),
                objective: None,
                max_turns: None,
                reason: None,
                subgoal: None,
                index_1based: None,
            };
            if let Ok(resp) = runtime.goal_action(status_probe).await {
                return Ok(resp);
            }
        }
        let Some(repo) = self.goal_repo_slot() else {
            return Ok(GoalStatusResponse::default());
        };
        Ok(repo
            .load_by_session(conversation_id)
            .await?
            .map(|row| nomifun_ai_agent::goal_bridge::goal_row_to_response(&row))
            .unwrap_or_default())
    }

    // ── Workspace browsing ──────────────────────────────────────────

    /// Enumerate entries under `query.path` inside the conversation's
    /// workspace root. Resolves the root from the conversation's
    /// `extra.workspace` and delegates the path-scoped listing (isolation
    /// guards + depth cap) to [`nomifun_file::list_workspace_level`].
    pub async fn browse_workspace(
        &self,
        user_id: &str,
        conversation_id: &str,
        query: WorkspaceBrowseQuery,
    ) -> Result<Vec<WorkspaceEntry>, AppError> {
        if query.path.trim().is_empty() {
            return Err(AppError::BadRequest("path must not be empty".into()));
        }

        let row = self
            .require_owned_conversation(user_id, conversation_id)
            .await?;

        let extra: serde_json::Value =
            serde_json::from_str(&row.extra).map_err(|e| AppError::Internal(format!("Invalid extra JSON: {e}")))?;
        let workspace = extra
            .get("workspace")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_owned();
        if workspace.is_empty() {
            return Err(AppError::BadRequest("Conversation has no workspace assigned".into()));
        }

        list_workspace_level(
            std::path::Path::new(&workspace),
            &query.path,
            query.search.as_deref(),
        )
    }
}
