use std::collections::{HashSet, VecDeque};

use nomifun_api_types::{
    ExecutionParticipant, LoopStopPolicy, PlannedExecution, PlannedExecutionStep,
    StepControlPolicy, VerificationPolicy,
};
use nomifun_common::{
    AgentExecutionStepId, AgentStepMode, AgentToolPolicy, AppError, ExecutionStepKind,
    ExecutionStepStatus, ParticipantAssignmentSource, StepFailurePolicy, MAX_AGENT_EXECUTION_STEPS,
};
use nomifun_db::{NewAgentExecutionStep, NewAgentExecutionStepDependency};

use crate::participant_router::{rank_participants, score_participant};

pub(crate) struct MaterializedPlan {
    pub steps: Vec<NewAgentExecutionStep>,
    pub dependencies: Vec<NewAgentExecutionStepDependency>,
}

pub(crate) fn materialize(
    plan: PlannedExecution,
    participants: &[ExecutionParticipant],
) -> Result<MaterializedPlan, AppError> {
    if plan.steps.is_empty() {
        return Err(AppError::BadRequest("execution plan must contain a step".to_owned()));
    }
    if plan.steps.len() > MAX_AGENT_EXECUTION_STEPS {
        return Err(AppError::BadRequest(format!(
            "execution plan exceeds {MAX_AGENT_EXECUTION_STEPS} steps"
        )));
    }
    validate_dag(&plan.steps)?;
    let tool_policies: Vec<AgentToolPolicy> = plan
        .steps
        .iter()
        .map(normalized_tool_policy)
        .collect();
    let failure_policies: Vec<StepFailurePolicy> = (0..plan.steps.len())
        .map(|index| normalized_failure_policy(&plan.steps, index))
        .collect();
    let active_participants: Vec<&ExecutionParticipant> = participants
        .iter()
        .filter(|participant| participant.retired_in_revision.is_none())
        .collect();
    if active_participants.is_empty()
        && plan
            .steps
            .iter()
            .any(|step| step.kind == ExecutionStepKind::Agent)
    {
        return Err(AppError::BadRequest(
            "an Agent step requires an active execution participant".to_owned(),
        ));
    }

    let step_ids: Vec<String> = (0..plan.steps.len())
        .map(|_| AgentExecutionStepId::new().into_string())
        .collect();
    let mut steps = Vec::with_capacity(plan.steps.len());
    let mut dependencies = Vec::new();
    for (index, planned) in plan.steps.into_iter().enumerate() {
        validate_step(&planned)?;
        let (participant_id, source, score, rationale) = if planned.kind == ExecutionStepKind::Agent {
            route(&planned, &active_participants)?
        } else {
            (None, None, None, None)
        };
        for blocker in planned.depends_on.iter().copied() {
            dependencies.push(NewAgentExecutionStepDependency {
                blocker_step_id: step_ids[blocker].clone(),
                blocked_step_id: step_ids[index].clone(),
            });
        }
        steps.push(NewAgentExecutionStep {
            step_id: step_ids[index].clone(),
            title: planned.title.trim().to_owned(),
            spec: planned.spec.trim().to_owned(),
            role: planned.role.map(|value| value.trim().to_owned()),
            tool_policy: tool_policies[index],
            kind: planned.kind,
            agent_mode: (planned.kind == ExecutionStepKind::Agent)
                .then(|| planned.agent_mode.unwrap_or(AgentStepMode::Normal)),
            profile: planned
                .profile
                .map(|profile| serde_json::to_string(&profile))
                .transpose()
                .map_err(|error| AppError::Internal(format!("encode step profile: {error}")))?,
            fanout_group: planned
                .fanout_group
                .map(|value| value.trim().to_owned()),
            control_policy: planned
                .control_policy
                .map(|policy| serde_json::to_string(&policy))
                .transpose()
                .map_err(|error| AppError::Internal(format!("encode control policy: {error}")))?,
            status: ExecutionStepStatus::Pending,
            assigned_participant_id: participant_id,
            assignment_score: score,
            assignment_rationale: rationale,
            assignment_source: source,
            assignment_locked: false,
            failure_policy: failure_policies[index],
            preset_prompt: None,
            graph_x: None,
            graph_y: None,
        });
    }
    Ok(MaterializedPlan {
        steps,
        dependencies,
    })
}

fn normalized_tool_policy(step: &PlannedExecutionStep) -> AgentToolPolicy {
    if step.kind != ExecutionStepKind::Agent {
        return AgentToolPolicy::Full;
    }
    if spec_requires_host_pipeline(&step.spec) {
        return AgentToolPolicy::Full;
    }
    match step.tool_policy {
        AgentToolPolicy::Full | AgentToolPolicy::FilesShell => AgentToolPolicy::FilesShell,
        AgentToolPolicy::ReadShell => AgentToolPolicy::ReadShell,
        AgentToolPolicy::ReadOnly => AgentToolPolicy::ReadOnly,
    }
}

fn spec_requires_host_pipeline(spec: &str) -> bool {
    const MARKERS: &[&str] = &[
        "briefing_create",
        "briefing_status",
        "news-briefing",
        "news_briefing",
        "art-motion",
        "art_motion",
        "nomi_delegate",
        "computer_use",
    ];
    let lower = spec.to_ascii_lowercase();
    MARKERS.iter().any(|marker| contains_tool_marker(&lower, marker))
}

fn contains_tool_marker(haystack: &str, marker: &str) -> bool {
    haystack.match_indices(marker).any(|(index, _)| {
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + marker.len()..].chars().next();
        !before.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
            && !after.is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn normalized_failure_policy(
    steps: &[PlannedExecutionStep],
    index: usize,
) -> StepFailurePolicy {
    let step = &steps[index];
    if spec_is_safety_gate(&step.spec) {
        return StepFailurePolicy::FailExecution;
    }
    let is_sink = !steps.iter().enumerate().any(|(other, candidate)| {
        other != index && candidate.depends_on.contains(&index)
    });
    let is_synthesis = step.agent_mode == Some(AgentStepMode::Synthesis);
    let is_last = index + 1 == steps.len();
    if is_sink && (is_synthesis || is_last) {
        StepFailurePolicy::FailExecution
    } else {
        StepFailurePolicy::SkipDependents
    }
}

fn spec_is_safety_gate(spec: &str) -> bool {
    let lower = spec.to_ascii_lowercase();
    [
        "safety",
        "destructive",
        "irreversible",
        "生产环境",
        "不可逆",
        "安全闸",
        "安全门",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn route(
    step: &PlannedExecutionStep,
    participants: &[&ExecutionParticipant],
) -> Result<
    (
        Option<String>,
        Option<ParticipantAssignmentSource>,
        Option<f64>,
        Option<String>,
    ),
    AppError,
> {
    if let Some(index) = step.participant_index {
        let participant = participants.get(index).ok_or_else(|| {
            AppError::BadRequest(format!(
                "step '{}' references participant index {index} outside the active snapshot",
                step.title
            ))
        })?;
        let viable = step
            .profile
            .as_ref()
            .is_none_or(|profile| score_participant(participant, profile).is_some());
        if !viable {
            return Err(AppError::BadRequest(format!(
                "step '{}' selects a participant that does not satisfy its profile",
                step.title
            )));
        }
        return Ok((
            Some(participant.participant_id.clone()),
            Some(ParticipantAssignmentSource::Planner),
            None,
            step.assignment_rationale.clone(),
        ));
    }

    if let Some(profile) = step.profile.as_ref() {
        let owned: Vec<ExecutionParticipant> = participants
            .iter()
            .map(|value| (*value).clone())
            .collect();
        if let Some(candidate) = rank_participants(&owned, profile).first() {
            return Ok((
                Some(owned[candidate.participant_index].participant_id.clone()),
                Some(ParticipantAssignmentSource::Automatic),
                Some(candidate.score),
                Some(candidate.rationale.clone()),
            ));
        }
        return Err(AppError::BadRequest(format!(
            "no participant satisfies the profile for step '{}'",
            step.title
        )));
    }
    let participant = participants
        .iter()
        .next()
        .ok_or_else(|| AppError::BadRequest("no participant can execute this step".to_owned()))?;
    Ok((
        Some(participant.participant_id.clone()),
        Some(ParticipantAssignmentSource::Automatic),
        None,
        Some("default active participant".to_owned()),
    ))
}

fn validate_step(step: &PlannedExecutionStep) -> Result<(), AppError> {
    if step.title.trim().is_empty() {
        return Err(AppError::BadRequest("step title must not be empty".to_owned()));
    }
    if step.spec.trim().is_empty() {
        return Err(AppError::BadRequest(format!(
            "step '{}' spec must not be empty",
            step.title
        )));
    }
    if step.role.as_ref().is_some_and(|role| role.trim().is_empty()) {
        return Err(AppError::BadRequest(format!(
            "step '{}' role must not be blank when present",
            step.title
        )));
    }
    if step
        .fanout_group
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::BadRequest(format!(
            "step '{}' fanout_group must not be empty when present",
            step.title
        )));
    }
    match (&step.kind, &step.agent_mode, &step.control_policy) {
        (ExecutionStepKind::Agent, _, None) => {}
        (
            ExecutionStepKind::Verify,
            None,
            Some(StepControlPolicy::Verify { vote }),
        ) if !step.depends_on.is_empty()
            && match vote {
                VerificationPolicy::AtLeast { count } => {
                    *count > 0 && *count <= step.depends_on.len()
                }
                VerificationPolicy::Majority | VerificationPolicy::Unanimous => true,
            } => {}
        (
            ExecutionStepKind::Judge,
            None,
            Some(StepControlPolicy::Judge { candidate_count, .. }),
        ) if !step.depends_on.is_empty()
            && candidate_count.is_none_or(|count| count > 0) => {}
        (
            ExecutionStepKind::Loop,
            None,
            Some(StepControlPolicy::Loop {
                max_iterations,
                stop,
            }),
        ) if step.depends_on.len() == 1
            && *max_iterations > 0
            && match stop {
                LoopStopPolicy::Predicate { done_marker } => !done_marker.trim().is_empty(),
                LoopStopPolicy::Stable { quiet_rounds } => *quiet_rounds > 0,
                LoopStopPolicy::MaxIterations | LoopStopPolicy::Approved => true,
            } => {}
        _ => {
            return Err(AppError::BadRequest(format!(
                "step '{}' has a kind/mode/control-policy mismatch",
                step.title
            )));
        }
    }
    if step.kind != ExecutionStepKind::Agent
        && (step.participant_index.is_some()
            || step.fanout_group.is_some()
            || step.profile.is_some()
            || step.assignment_rationale.is_some()
            || step.tool_policy != AgentToolPolicy::Full)
    {
        return Err(AppError::BadRequest(format!(
            "control step '{}' cannot declare Agent routing fields",
            step.title
        )));
    }
    Ok(())
}

fn validate_dag(steps: &[PlannedExecutionStep]) -> Result<(), AppError> {
    let mut outgoing = vec![Vec::new(); steps.len()];
    let mut indegree = vec![0_usize; steps.len()];
    for (blocked, step) in steps.iter().enumerate() {
        let mut unique = HashSet::new();
        for blocker in step.depends_on.iter().copied() {
            if blocker >= steps.len() || blocker == blocked {
                return Err(AppError::BadRequest(format!(
                    "step {blocked} has an invalid dependency index {blocker}"
                )));
            }
            if !unique.insert(blocker) {
                return Err(AppError::BadRequest(format!(
                    "step {blocked} repeats dependency index {blocker}"
                )));
            }
            outgoing[blocker].push(blocked);
            indegree[blocked] += 1;
        }
    }
    let mut ready: VecDeque<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut visited = 0;
    while let Some(index) = ready.pop_front() {
        visited += 1;
        for blocked in outgoing[index].iter().copied() {
            indegree[blocked] -= 1;
            if indegree[blocked] == 0 {
                ready.push_back(blocked);
            }
        }
    }
    if visited != steps.len() {
        return Err(AppError::BadRequest("execution plan contains a cycle".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_is_rejected_before_persistence() {
        let step = |depends_on| PlannedExecutionStep {
            title: "x".to_owned(),
            spec: "x".to_owned(),
            profile: None,
            kind: ExecutionStepKind::Agent,
            agent_mode: Some(AgentStepMode::Normal),
            depends_on,
            participant_index: Some(0),
            assignment_rationale: None,
            role: None,
            tool_policy: AgentToolPolicy::Full,
            fanout_group: None,
            control_policy: None,
            failure_policy: nomifun_common::StepFailurePolicy::FailExecution,
        };
        assert!(validate_dag(&[step(vec![1]), step(vec![0])]).is_err());
    }

    #[test]
    fn free_form_role_never_changes_explicit_tool_authority() {
        let step = |role: &str, tool_policy| PlannedExecutionStep {
            title: role.to_owned(),
            spec: "test".to_owned(),
            profile: None,
            kind: ExecutionStepKind::Agent,
            agent_mode: Some(AgentStepMode::Normal),
            depends_on: vec![],
            participant_index: Some(0),
            assignment_rationale: None,
            role: Some(role.to_owned()),
            tool_policy,
            fanout_group: None,
            control_policy: None,
            failure_policy: nomifun_common::StepFailurePolicy::FailExecution,
        };

        for role in ["builder", "implementer", "后端", "custom-domain-expert"] {
            let planned = step(role, AgentToolPolicy::Full);
            assert!(validate_step(&planned).is_ok(), "free-form role {role}");
            assert_eq!(planned.tool_policy, AgentToolPolicy::Full);
        }
        let narrowed = step("builder", AgentToolPolicy::ReadOnly);
        assert!(validate_step(&narrowed).is_ok());
        assert_eq!(narrowed.tool_policy, AgentToolPolicy::ReadOnly);
    }

    fn planned(
        spec: &str,
        tool_policy: AgentToolPolicy,
        depends_on: Vec<usize>,
    ) -> PlannedExecutionStep {
        PlannedExecutionStep {
            title: "step".to_owned(),
            spec: spec.to_owned(),
            profile: None,
            kind: ExecutionStepKind::Agent,
            agent_mode: Some(AgentStepMode::Normal),
            depends_on,
            participant_index: Some(0),
            assignment_rationale: None,
            role: None,
            tool_policy,
            fanout_group: None,
            control_policy: None,
            failure_policy: StepFailurePolicy::FailExecution,
        }
    }

    #[test]
    fn host_pipeline_specs_cannot_stay_read_only() {
        let news = planned(
            "核实当日新闻。使用 news-briefing / briefing_create，禁止 web_search。",
            AgentToolPolicy::ReadOnly,
            vec![],
        );
        assert_eq!(normalized_tool_policy(&news), AgentToolPolicy::Full);
    }

    #[test]
    fn ordinary_implementation_narrows_full_to_files_shell() {
        let env = planned(
            "初始化工程，复制引擎并确认 `render.py` 可运行。",
            AgentToolPolicy::Full,
            vec![],
        );
        assert_eq!(normalized_tool_policy(&env), AgentToolPolicy::FilesShell);
        let script = planned("写入 `口播脚本.md`", AgentToolPolicy::Full, vec![0]);
        assert_eq!(normalized_tool_policy(&script), AgentToolPolicy::FilesShell);
    }

    #[test]
    fn middle_steps_skip_dependents_final_sink_fails_execution() {
        let steps = vec![
            planned("初始化环境", AgentToolPolicy::Full, vec![]),
            planned("核实新闻 briefing_create", AgentToolPolicy::Full, vec![]),
            planned("写入口播稿", AgentToolPolicy::Full, vec![1]),
        ];
        assert_eq!(
            normalized_failure_policy(&steps, 0),
            StepFailurePolicy::SkipDependents
        );
        assert_eq!(
            normalized_failure_policy(&steps, 1),
            StepFailurePolicy::SkipDependents
        );
        assert_eq!(
            normalized_failure_policy(&steps, 2),
            StepFailurePolicy::FailExecution
        );
    }
}
