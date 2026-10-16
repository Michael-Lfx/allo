//! Internal planning strategy for [`AgentExecutionEngine`](crate::AgentExecutionEngine).
//!
//! Planner output uses the same typed step vocabulary as persistence. There is
//! no second bag of `kind + pattern_config`, so model output, HTTP input and DB
//! validation cannot drift into subtly different execution modes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use nomifun_ai_agent::{
    DeltaKind, resolve_provider_config, streaming_completion_text_or_reasoning, user_message,
};
use nomifun_api_types::{
    AgentExecutionDetail, ExecutionParticipant, PlannedExecution, PlannedExecutionStep,
};
use nomifun_common::{
    AgentStepMode, AgentToolPolicy, AppError, ExecutionStepKind, ProviderId, ProviderWithModel,
    StepFailurePolicy,
};
use nomifun_db::{IProviderModelRepository, IProviderRepository, ProviderModelRow};

use crate::event_publisher::{AgentExecutionEventPublisher, LeadThinkingKind, LeadThinkingPhase};

const PLAN_MAX_TOKENS: u32 = 8192;
const FALLBACK_TITLE_LEN: usize = 60;
const PLAN_FALLBACK_NOTICE: &str = "\n\n⚠️ 自动拆解没有返回有效计划，已回退为一个可执行步骤。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeadDeltaKind {
    Text,
    Reasoning,
}

impl From<DeltaKind> for LeadDeltaKind {
    fn from(value: DeltaKind) -> Self {
        match value {
            DeltaKind::Text => Self::Text,
            DeltaKind::Reasoning => Self::Reasoning,
        }
    }
}

pub(crate) type LeadThinkingSink = Arc<dyn Fn(LeadDeltaKind, &str) + Send + Sync>;

const THROTTLE_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(80);
const THROTTLE_FLUSH_CHARS: usize = 48;

#[derive(Default)]
struct ThrottleBuffer {
    reasoning: String,
    text: String,
    reasoning_last: Option<std::time::Instant>,
    text_last: Option<std::time::Instant>,
}

/// Coalesces lead-model deltas while keeping the durable execution event stream
/// free of high-frequency token traffic.
pub(crate) struct LeadThinkingThrottle {
    publisher: AgentExecutionEventPublisher,
    owner_id: String,
    execution_id: String,
    phase: LeadThinkingPhase,
    buffer: Arc<std::sync::Mutex<ThrottleBuffer>>,
}

impl LeadThinkingThrottle {
    pub fn new(
        publisher: AgentExecutionEventPublisher,
        owner_id: impl Into<String>,
        execution_id: impl Into<String>,
        phase: LeadThinkingPhase,
    ) -> Self {
        Self {
            publisher,
            owner_id: owner_id.into(),
            execution_id: execution_id.into(),
            phase,
            buffer: Arc::new(std::sync::Mutex::new(ThrottleBuffer::default())),
        }
    }

    pub fn sink(&self) -> LeadThinkingSink {
        let publisher = self.publisher.clone();
        let owner_id = self.owner_id.clone();
        let execution_id = self.execution_id.clone();
        let phase = self.phase;
        let buffer = Arc::clone(&self.buffer);
        Arc::new(move |kind, delta| {
            if delta.is_empty() {
                return;
            }
            let now = std::time::Instant::now();
            let emit = {
                let Ok(mut guard) = buffer.lock() else {
                    return;
                };
                let ThrottleBuffer {
                    reasoning,
                    text,
                    reasoning_last,
                    text_last,
                } = &mut *guard;
                let (pending, last, wire_kind) = match kind {
                    LeadDeltaKind::Reasoning => {
                        (reasoning, reasoning_last, LeadThinkingKind::Reasoning)
                    }
                    LeadDeltaKind::Text => (text, text_last, LeadThinkingKind::Text),
                };
                pending.push_str(delta);
                let due = last
                    .map(|instant| now.duration_since(instant) >= THROTTLE_FLUSH_INTERVAL)
                    .unwrap_or(true)
                    || pending.chars().count() >= THROTTLE_FLUSH_CHARS;
                if due {
                    *last = Some(now);
                    Some((wire_kind, std::mem::take(pending)))
                } else {
                    None
                }
            };
            if let Some((wire_kind, chunk)) = emit {
                publisher.publish_lead_thinking(
                    &owner_id,
                    &execution_id,
                    phase,
                    wire_kind,
                    Some(&chunk),
                    None,
                    false,
                );
            }
        })
    }

    pub fn flush(&self) {
        let (reasoning, text) = {
            let Ok(mut guard) = self.buffer.lock() else {
                return;
            };
            (
                std::mem::take(&mut guard.reasoning),
                std::mem::take(&mut guard.text),
            )
        };
        if !reasoning.is_empty() {
            self.publisher.publish_lead_thinking(
                &self.owner_id,
                &self.execution_id,
                self.phase,
                LeadThinkingKind::Reasoning,
                Some(&reasoning),
                None,
                false,
            );
        }
        if !text.is_empty() {
            self.publisher.publish_lead_thinking(
                &self.owner_id,
                &self.execution_id,
                self.phase,
                LeadThinkingKind::Text,
                Some(&text),
                None,
                false,
            );
        }
    }
}

#[async_trait]
pub(crate) trait PlanProducer: Send + Sync {
    async fn produce(
        &self,
        goal: &str,
        participants: &[ExecutionParticipant],
        sink: Option<&LeadThinkingSink>,
    ) -> Result<PlannedExecution, AppError>;

    async fn adjust(
        &self,
        _intent: &str,
        _current: &AgentExecutionDetail,
        _sink: Option<&LeadThinkingSink>,
    ) -> Result<AdjustedExecutionPlan, AppError> {
        Err(AppError::BadRequest(
            "the configured planner does not support execution adjustment".to_owned(),
        ))
    }

}

pub(crate) struct LlmPlanProducer {
    provider_repo: Arc<dyn IProviderRepository>,
    provider_model_repo: Arc<dyn IProviderModelRepository>,
    encryption_key: [u8; 32],
    workspace: PathBuf,
}

impl LlmPlanProducer {
    pub fn new(
        provider_repo: Arc<dyn IProviderRepository>,
        provider_model_repo: Arc<dyn IProviderModelRepository>,
        encryption_key: [u8; 32],
        workspace: impl Into<PathBuf>,
    ) -> Self {
        Self {
            provider_repo,
            provider_model_repo,
            encryption_key,
            workspace: workspace.into(),
        }
    }

    async fn complete(
        &self,
        participants: &[ExecutionParticipant],
        system: &str,
        user: String,
        max_tokens: u32,
        sink: Option<&LeadThinkingSink>,
    ) -> Result<String, AppError> {
        let lead = pick_lead(participants).ok_or_else(|| {
            AppError::ProviderUnavailable(
                "execution planner has no canonical provider/model participant".to_owned(),
            )
        })?;
        let model = lead.use_model.as_deref().unwrap_or(&lead.model);
        let config = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            &lead.provider_id,
            model,
            self.workspace.as_path(),
        )
        .await?;
        streaming_completion_text_or_reasoning(
            &config,
            system,
            vec![user_message(user)],
            max_tokens,
            |kind, delta| {
                if let Some(sink) = sink {
                    sink(kind.into(), delta);
                }
            },
        )
        .await
    }
}

#[async_trait]
impl PlanProducer for LlmPlanProducer {
    async fn produce(
        &self,
        goal: &str,
        participants: &[ExecutionParticipant],
        sink: Option<&LeadThinkingSink>,
    ) -> Result<PlannedExecution, AppError> {
        let descriptions = match self.provider_model_repo.list().await {
            Ok(rows) => build_description_map(&rows, participants),
            Err(error) => {
                tracing::warn!(%error, "planning without provider model descriptions");
                HashMap::new()
            }
        };
        let raw = self
            .complete(
                participants,
                PLAN_SYSTEM,
                build_plan_user_prompt(goal, participants, &descriptions),
                PLAN_MAX_TOKENS,
                sink,
            )
            .await?;
        let plan = if let Some(plan) = parse_plan_opt(&raw) {
            plan
        } else {
            tracing::warn!(raw_len = raw.len(), "planner output invalid; using one-step plan");
            if let Some(sink) = sink {
                sink(LeadDeltaKind::Text, PLAN_FALLBACK_NOTICE);
            }
            fallback_plan(goal)
        };
        Ok(canonicalize_plan_for_goal(goal, plan))
    }

    async fn adjust(
        &self,
        intent: &str,
        current: &AgentExecutionDetail,
        sink: Option<&LeadThinkingSink>,
    ) -> Result<AdjustedExecutionPlan, AppError> {
        let raw = self
            .complete(
                &current.participants,
                ADJUST_SYSTEM,
                build_adjust_user_prompt(intent, current),
                PLAN_MAX_TOKENS,
                sink,
            )
            .await?;
        parse_adjusted_plan(&raw)
    }

}

fn pick_lead(participants: &[ExecutionParticipant]) -> Option<ProviderWithModel> {
    participants.iter().find_map(|participant| {
        let provider_id = participant.provider_id.as_ref()?;
        let model = participant.model.as_ref()?;
        (ProviderId::try_from(provider_id.as_str()).is_ok()
            && !model.is_empty()
            && model.trim() == model)
            .then(|| ProviderWithModel {
                provider_id: provider_id.clone(),
                model: model.clone(),
                use_model: Some(model.clone()),
            })
    })
}

const PLAN_SYSTEM: &str = r#"You are the lead Agent planning one AgentExecution.
Return ONLY strict JSON with this shape:
{"steps":[{"title":"...","spec":"...","profile":null,"kind":"agent","agent_mode":"normal","depends_on":[],"participant_index":0,"assignment_rationale":"...","role":"...","tool_policy":"files_shell","fanout_group":null,"control_policy":null,"failure_policy":"skip_dependents"}]}

Rules:
- depends_on contains only zero-based indices of earlier steps; keep the DAG acyclic and minimal.
- Normal and synthesis work both use kind=agent. Use agent_mode=normal for ordinary work and synthesis only to merge dependency outputs.
- Parallel variants are ordinary agent steps sharing fanout_group; a later synthesis step depends on all variants.
- kind=verify is a no-Agent vote controller. Its control_policy is {"kind":"verify","vote":{"mode":"majority"}}, {"kind":"verify","vote":{"mode":"unanimous"}}, or {"kind":"verify","vote":{"mode":"at_least","count":2}}. It depends on Agent critics that emit {"pass":true|false,"critique":"..."}.
- kind=judge is a no-Agent ballot controller. Its control_policy is {"kind":"judge","aggregation":"mean","candidate_count":3} or aggregation=borda. It depends on Agent judges that score candidates.
- kind=loop is a no-Agent iteration controller. Its control_policy is {"kind":"loop","max_iterations":4,"stop":{"kind":"max_iterations"}}, predicate(done_marker), stable(quiet_rounds), or approved. It has exactly one body dependency.
- Controller steps have agent_mode=null and participant_index=null. Agent steps have control_policy=null and a participant_index when a particular participant is preferred.
- Default failure_policy=skip_dependents so a failed step only skips its dependents; independent work continues. Use fail_execution only for the final deliverable, a synthesis merge, or a safety/destructive gate.
- Assign cheap/fast participants to simple or bulk work and stronger participants to difficult reasoning. Do not route everything to the strongest model.
- role is an optional short human-readable description of the work, in the goal's language. It is never a permission value.
- tool_policy is exactly full, files_shell, read_only, or read_shell. A policy only narrows the caller's inherited authority.
- If the spec names a host pipeline (briefing_create, briefing_status, news-briefing, art-motion, nomi_delegate), tool_policy MUST be full. Never give that work read_only — those tools will be missing.
- If the GOAL is sourced news briefing / 新闻口播 / news+TTS video, return exactly one Agent step with tool_policy=full whose spec tells the worker to call briefing_create and briefing_status. Do not invent a research/write/render/art-motion DAG, and never assign that work read_only. art-motion is only an optional follow-on after briefing produces narration audio.
- Use files_shell for implementation that only needs files and a shell (Read/Write/Edit/Grep/Glob/DirTree/Bash/web_search). Use read_only for research/review that must not modify files and does not need host pipelines. Use read_shell for verification/testing that also needs Bash. Controller steps use full.
- Prefer calling an already-available host primitive over rebuilding the same pipeline as extra DAG steps.
- title is short; spec is the complete instruction.
- Use advanced patterns only when they materially improve the result.
"#;


const ADJUST_SYSTEM: &str = r#"You are revising an existing AgentExecution from a user instruction. Return ONLY strict JSON: {"steps":[...]}. Each item is either {"type":"keep","step_id":"existing-id"} or {"type":"new","step":<the same typed step object used by planning>,"dependencies":[{"type":"existing","step_id":"..."}|{"type":"new","index":0}]}. Keep completed work that remains useful, omit obsolete work, add only needed work, and keep the resulting graph acyclic. Never invent an existing id. A new Agent step role is an optional short human-readable work description and never grants tools. Set its explicit tool_policy to full, files_shell, read_only, or read_shell using the same rules as planning. Specs that name briefing_create, news-briefing, or art-motion must use full. A sourced news briefing / 新闻口播 goal must stay a single full briefing_create step rather than a research DAG. Default failure_policy is skip_dependents except for the final deliverable or a safety gate."#;

type DescriptionMap = HashMap<(String, String), String>;

fn build_description_map(
    model_rows: &[ProviderModelRow],
    participants: &[ExecutionParticipant],
) -> DescriptionMap {
    // Per-model descriptions live on the provider_models rows (migration 016).
    let by_key: HashMap<(&str, &str), &str> = model_rows
        .iter()
        .filter_map(|row| {
            let description = row.description.as_deref().map(str::trim)?;
            (!description.is_empty()).then_some((
                (row.provider_id.as_str(), row.model.as_str()),
                description,
            ))
        })
        .collect();
    let mut descriptions = HashMap::new();
    for participant in participants {
        let (Some(provider_id), Some(model)) = (
            participant.provider_id.as_deref(),
            participant.model.as_deref(),
        ) else {
            continue;
        };
        if let Some(description) = by_key.get(&(provider_id, model)) {
            descriptions.insert(
                (provider_id.to_owned(), model.to_owned()),
                (*description).to_owned(),
            );
        }
    }
    descriptions
}

fn build_plan_user_prompt(
    goal: &str,
    participants: &[ExecutionParticipant],
    descriptions: &DescriptionMap,
) -> String {
    let mut prompt = format!("GOAL:\n{}\n\nPARTICIPANTS:\n", goal.trim());
    if participants.is_empty() {
        prompt.push_str("(none)\n");
    }
    for (index, participant) in participants.iter().enumerate() {
        let strengths = participant
            .capability
            .as_ref()
            .map(|capability| capability.strengths.join("/"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "-".to_owned());
        let description = participant
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                Some(descriptions.get(&(
                    participant.provider_id.clone()?,
                    participant.model.clone()?,
                ))?
                .as_str())
            })
            .unwrap_or("-");
        prompt.push_str(&format!(
            "{index}. agent={} role={} model={} strengths={} description={}\n",
            participant.source_agent_id,
            participant.role.as_deref().unwrap_or("-"),
            participant.model.as_deref().unwrap_or("-"),
            strengths,
            description,
        ));
    }
    prompt.push_str("\nReturn the typed JSON plan only.");
    prompt
}

#[cfg(test)]
fn parse_plan(raw: &str, goal: &str) -> PlannedExecution {
    parse_plan_opt(raw).unwrap_or_else(|| fallback_plan(goal))
}

pub(crate) fn parse_plan_opt(raw: &str) -> Option<PlannedExecution> {
    let plan: PlannedExecution = serde_json::from_str(&extract_json_object(raw)?).ok()?;
    (!plan.steps.is_empty()).then_some(plan)
}

fn fallback_plan(goal: &str) -> PlannedExecution {
    canonicalize_plan_for_goal(
        goal,
        PlannedExecution {
            steps: vec![PlannedExecutionStep {
                title: truncate_title(goal),
                spec: goal.trim().to_owned(),
                profile: None,
                kind: ExecutionStepKind::Agent,
                agent_mode: Some(AgentStepMode::Normal),
                depends_on: vec![],
                participant_index: Some(0),
                assignment_rationale: Some("planner fallback".to_owned()),
                role: None,
                tool_policy: AgentToolPolicy::Full,
                fanout_group: None,
                control_policy: None,
                failure_policy: StepFailurePolicy::SkipDependents,
            }],
        },
    )
}

/// Sourced news/口播 video is a host pipeline, not a generic research DAG.
/// Collapsing here is defense in depth when a parent still delegated.
fn canonicalize_plan_for_goal(goal: &str, plan: PlannedExecution) -> PlannedExecution {
    if !goal_is_news_briefing(goal) {
        return plan;
    }
    if plan.steps.len() == 1 && spec_names_news_briefing(&plan.steps[0].spec) {
        let mut plan = plan;
        plan.steps[0].tool_policy = AgentToolPolicy::Full;
        plan.steps[0].failure_policy = StepFailurePolicy::FailExecution;
        return plan;
    }
    host_pipeline_news_briefing_plan(goal)
}

fn goal_is_news_briefing(goal: &str) -> bool {
    let lower = goal.to_lowercase();
    let news = ["新闻", "资讯", "briefing", "news"]
        .iter()
        .any(|marker| lower.contains(marker));
    let spoken_or_video = ["口播", "配音", "tts", "narrat", "视频", "video"]
        .iter()
        .any(|marker| lower.contains(marker));
    news && spoken_or_video
}

fn spec_names_news_briefing(spec: &str) -> bool {
    let lower = spec.to_ascii_lowercase();
    ["briefing_create", "news-briefing", "news_briefing"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn host_pipeline_news_briefing_plan(goal: &str) -> PlannedExecution {
    PlannedExecution {
        steps: vec![PlannedExecutionStep {
            title: "生成当日新闻口播".to_owned(),
            spec: format!(
                "用户目标：{}\n\n\
                 必须直接调用 `briefing_create`（intent 用用户目标，format_secs 按用户指定否则 90）\
                 并轮询 `briefing_status` 直到完成。禁止 web_search 自制新闻稿，禁止 nomi_delegate 再拆 DAG，\
                 禁止用 art-motion 从零复制渲染引擎。若用户还要求艺术动画，等 briefing 产出旁白音频与脚本后再作为画面增强。\
                 成功后把 briefing 会话与成片路径回报给用户。",
                goal.trim()
            ),
            profile: None,
            kind: ExecutionStepKind::Agent,
            agent_mode: Some(AgentStepMode::Normal),
            depends_on: vec![],
            participant_index: Some(0),
            assignment_rationale: Some("host pipeline: news-briefing".to_owned()),
            role: Some("news-briefing".to_owned()),
            tool_policy: AgentToolPolicy::Full,
            fanout_group: None,
            control_policy: None,
            failure_policy: StepFailurePolicy::FailExecution,
        }],
    }
}

fn truncate_title(goal: &str) -> String {
    let goal = goal.trim();
    if goal.chars().count() <= FALLBACK_TITLE_LEN {
        goal.to_owned()
    } else {
        format!("{}…", goal.chars().take(FALLBACK_TITLE_LEN).collect::<String>())
    }
}

fn extract_json_object(raw: &str) -> Option<String> {
    let cleaned = raw
        .replace("```json", "")
        .replace("```JSON", "")
        .replace("```", "");
    let bytes = cleaned.as_bytes();
    let start = cleaned.find('{')?;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for index in start..bytes.len() {
        let current = bytes[index] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            continue;
        }
        match current {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cleaned[start..=index].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AdjustedExecutionNode {
    Keep { step_id: String },
    New {
        step: PlannedExecutionStep,
        #[serde(default)]
        dependencies: Vec<AdjustedDependency>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AdjustedDependency {
    Existing { step_id: String },
    New { index: usize },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdjustedExecutionPlan {
    pub steps: Vec<AdjustedExecutionNode>,
}

pub(crate) fn parse_adjusted_plan(raw: &str) -> Result<AdjustedExecutionPlan, AppError> {
    let object = extract_json_object(raw).ok_or_else(|| {
        AppError::BadRequest("主 Agent 的调整计划没有返回 JSON，执行未改动".to_owned())
    })?;
    let plan: AdjustedExecutionPlan = serde_json::from_str(&object).map_err(|error| {
        AppError::BadRequest(format!("主 Agent 的调整计划无效（{error}），执行未改动"))
    })?;
    if plan.steps.is_empty() {
        return Err(AppError::BadRequest(
            "调整计划不能删除全部步骤".to_owned(),
        ));
    }
    Ok(plan)
}

fn build_adjust_user_prompt(intent: &str, detail: &AgentExecutionDetail) -> String {
    let mut prompt = format!(
        "INTENT:\n{}\n\nCURRENT EXECUTION (id | title | kind | status | dependencies | latest output):\n",
        intent.trim()
    );
    for step in &detail.steps {
        let dependencies: Vec<&str> = detail
            .dependencies
            .iter()
            .filter(|dependency| dependency.blocked_step_id == step.step_id)
            .map(|dependency| dependency.blocker_step_id.as_str())
            .collect();
        let output = detail
            .attempts
            .iter()
            .filter(|attempt| attempt.step_id == step.step_id)
            .max_by_key(|attempt| attempt.attempt_no)
            .and_then(|attempt| attempt.output_summary.as_deref())
            .map(compact_output)
            .unwrap_or_else(|| "-".to_owned());
        prompt.push_str(&format!(
            "{} | {} | {} | {} | {:?} | {}\n",
            step.step_id, step.title, step.kind, step.status, dependencies, output
        ));
    }
    prompt.push_str("\nReturn the adjusted JSON plan only.");
    prompt
}

fn compact_output(output: &str) -> String {
    let one_line = output.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 300 {
        one_line
    } else {
        format!("{}…", one_line.chars().take(300).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_typed_plan_and_rejects_removed_config_bag() {
        let valid = r#"{"steps":[{"title":"A","spec":"do A","kind":"agent","agent_mode":"normal","depends_on":[],"failure_policy":"fail_execution"}]}"#;
        assert!(parse_plan_opt(valid).is_some());

        let removed = r#"{"steps":[{"title":"A","spec":"do A","kind":"agent","pattern_config":"{}","depends_on":[]}]}"#;
        assert!(parse_plan_opt(removed).is_none());
    }

    #[test]
    fn fallback_is_one_normal_agent_step() {
        let plan = parse_plan("not json", "ship it");
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].agent_mode, Some(AgentStepMode::Normal));
    }

    #[test]
    fn news_briefing_goal_collapses_a_research_dag_to_one_full_host_step() {
        let raw = r#"{"steps":[
            {"title":"核实新闻","spec":"用 web_search 采集当日新闻","kind":"agent","agent_mode":"normal","depends_on":[],"tool_policy":"read_only","failure_policy":"fail_execution"},
            {"title":"写口播稿","spec":"写入口播脚本.md","kind":"agent","agent_mode":"normal","depends_on":[0],"tool_policy":"full","failure_policy":"fail_execution"}
        ]}"#;
        let parsed = parse_plan_opt(raw).expect("typed plan");
        let plan = canonicalize_plan_for_goal("做个当日新闻资讯的口播视频吧", parsed);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].tool_policy, AgentToolPolicy::Full);
        assert!(plan.steps[0].spec.contains("briefing_create"));
        assert!(!plan.steps[0].spec.contains("web_search 采集"));
    }

    #[test]
    fn ordinary_goals_keep_the_parsed_dag() {
        let raw = r#"{"steps":[
            {"title":"A","spec":"do A","kind":"agent","agent_mode":"normal","depends_on":[],"tool_policy":"files_shell"},
            {"title":"B","spec":"do B","kind":"agent","agent_mode":"normal","depends_on":[0],"tool_policy":"files_shell"}
        ]}"#;
        let parsed = parse_plan_opt(raw).expect("typed plan");
        let plan = canonicalize_plan_for_goal("refactor the parser", parsed);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].title, "A");
    }
}
