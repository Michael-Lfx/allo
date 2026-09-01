//! Lesson content draft + deterministic audit — the lesson sibling of
//! `course_outline/draft.rs`. The agent builds one lesson's content through
//! patch ops (`set_document` / activity ops), every patch re-runs the
//! deterministic audit, and the finish gate re-audits live before the draft
//! converts into the generation stage's [`LessonOutput`].
//!
//! The audit reuses the fallback pipeline's rules verbatim — the document
//! contract (`validate_lesson_document`: length floor + three required
//! sections in order) and the activity rules (`validate_lesson_activities`:
//! count floors, reflection cap, concept binding, per-kind shape) — inlined
//! with position-aware messages so one audit pass surfaces every finding.

use std::collections::HashSet;

use crate::generation::{
    LESSON_MAX_REFLECTION_ACTIVITIES, LESSON_MIN_ACTIVITIES, LESSON_MIN_OBJECTIVE_ACTIVITIES,
    LessonOutput, validate_lesson_document,
};
use crate::models::{ActivityKind, ActivityPack, ConceptPack};

use crate::learning_graph::{SEV_DANGER, SEV_WARNING};

/// Hard cap on the activities a draft may hold: the design guidance is 3-5,
/// so anything near this cap is already off-script (the audit warns before
/// it ever blocks).
const MAX_ACTIVITIES: usize = 10;

/// Grounding excerpt for the kb flow: the sampled file the lesson cites.
#[derive(Debug, Clone)]
pub struct LessonExcerpt {
    pub path: String,
    pub text: String,
}

/// 学习图课程节点专属的生成上下文（beta）。传统课时恒为 `None`；为 Some
/// 时，引擎提示词以图语义段落（学习目标/学习范围/前置路径/后续节点）取代
/// 课程大纲段，保证节点内容有全局观、范围受限、并能顺畅衔接后续节点。
#[derive(Debug, Clone)]
pub struct GraphLessonContext {
    /// 用户生成图时输入的学习目标。
    pub goal: String,
    /// 学习图的学习范围（scope 分析文本）。
    pub scope: String,
    /// 预渲染：到达此节点的前置整条路径（祖先闭包，按拓扑序排列；离节点
    /// 最近的前置优先展示，超长截断），让生成知道「学习者此刻已经会什么」。
    /// 节点内容是共享课程资产，不含任何用户个人进度。
    pub prerequisite_path: String,
    /// 预渲染：此节点之后的下游节点（直接后继全列 + 可及后代总数），
    /// 让生成知道「要为哪些后续学习做衔接」。
    pub upcoming_nodes: String,
}

/// The lesson context an engine generates content for: course/module/lesson
/// coordinates, the lesson's concepts, and the grounding (the cited excerpt
/// for the kb flow, the course brief for the description flow). Built by
/// `generate_lesson_content` from the persisted course snapshot.
#[derive(Debug, Clone)]
pub struct LessonGenerationContext {
    pub course_title: String,
    /// Description-flow grounding (kb-flow lessons ground in `excerpt`).
    pub course_description: String,
    pub module_title: String,
    pub module_index: usize,
    pub lesson_title: String,
    pub lesson_index: usize,
    pub total_lessons: usize,
    pub next_lesson_title: Option<String>,
    pub purpose: String,
    /// Concept packs bound to this lesson (prompt display).
    pub concepts: Vec<ConceptPack>,
    /// The exact keys activities may bind — the audit rejects anything else.
    pub concept_keys: Vec<String>,
    /// Kb-flow grounding; `None` on the description flow.
    pub excerpt: Option<LessonExcerpt>,
    /// Pre-rendered full course outline with the current lesson marked — the
    /// anti-duplication / no-scope-creep reference shared by both pipelines.
    pub outline_tree: String,
    /// Pre-rendered prev/next lesson reference (title/purpose/truncated
    /// excerpt); empty when there is nothing to reference.
    pub adjacent_context: String,
    /// 学习图节点专属上下文；传统课时恒为 `None`。
    pub graph: Option<GraphLessonContext>,
}

/// One deterministic audit finding. `severity` uses the shared vocabulary
/// (`SEV_DANGER` blocks the finish gate, `SEV_WARNING` is advisory).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LessonFinding {
    pub severity: &'static str,
    pub kind: String,
    pub message: String,
}

/// One lesson's in-memory draft. Short-lived (same lifecycle as the outline
/// drafts): `finish_lesson_draft` is the single publish path.
#[derive(Debug, Clone)]
pub struct LessonDraft {
    pub context: LessonGenerationContext,
    /// The study document (plain Markdown, no JSON wrapper) — `None` until
    /// `set_document` runs.
    pub document: Option<String>,
    pub estimated_minutes: i64,
    pub activities: Vec<ActivityPack>,
    /// Bumps once per accepted op — the repair loop's "did the model touch
    /// anything" signal.
    pub revision: usize,
    pub findings: Vec<LessonFinding>,
}

/// Patch ops for a lesson draft. Tagged `{"op": "..."}` over the wire like
/// the outline ops. Activity positions are 0-based indices into the list at
/// the moment the op applies; ops execute in array order.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LessonOp {
    SetDocument {
        document: String,
    },
    SetEstimatedMinutes {
        minutes: i64,
    },
    AddActivity {
        activity: ActivityPack,
    },
    UpdateActivity {
        position: usize,
        activity: ActivityPack,
    },
    RemoveActivity {
        position: usize,
    },
}

impl LessonOp {
    fn name(&self) -> &'static str {
        match self {
            Self::SetDocument { .. } => "set_document",
            Self::SetEstimatedMinutes { .. } => "set_estimated_minutes",
            Self::AddActivity { .. } => "add_activity",
            Self::UpdateActivity { .. } => "update_activity",
            Self::RemoveActivity { .. } => "remove_activity",
        }
    }
}

/// Draft snapshot returned by `ls_start` — everything the model needs to
/// start working without re-asking for context.
#[derive(Debug, serde::Serialize)]
pub struct LessonDraftView {
    pub draft_id: String,
    pub course_title: String,
    pub module_title: String,
    pub lesson_title: String,
    pub purpose: String,
    pub concept_keys: Vec<String>,
    /// `excerpt:<path>` for the kb flow, `course_brief` for the description
    /// flow (the grounding text itself rode in on the engine's user turn).
    pub grounding: String,
    pub revision: usize,
}

/// One activity in the inspect view: position + kind + prompt head, so the
/// model can address `update_activity`/`remove_activity` precisely.
#[derive(Debug, serde::Serialize)]
pub struct LessonActivitySummary {
    pub position: usize,
    pub kind: String,
    pub prompt: String,
}

/// Overview for `ls_inspect`: document shape, activity list and the fresh
/// audit findings.
#[derive(Debug, serde::Serialize)]
pub struct LessonInspectView {
    pub revision: usize,
    pub document_written: bool,
    /// Non-whitespace characters — the same counting the length floor uses.
    pub document_chars: usize,
    /// The document's `## ` heading lines in order.
    pub document_sections: Vec<String>,
    pub estimated_minutes: i64,
    pub activities: Vec<LessonActivitySummary>,
    pub findings: Vec<LessonFinding>,
}

/// One rejected op with its reason (accepted ops summarize what changed).
#[derive(Debug, serde::Serialize)]
pub struct LessonRejectedOp {
    pub op: String,
    pub reason: String,
}

/// Per-op verdicts plus a fresh audit snapshot — the `ls_patch_activities`
/// / `ls_set_document` return shape.
#[derive(Debug, serde::Serialize)]
pub struct LessonPatchReport {
    pub revision: usize,
    pub accepted: Vec<String>,
    pub rejected: Vec<LessonRejectedOp>,
    pub findings: Vec<LessonFinding>,
}

impl LessonDraft {
    pub fn new(context: LessonGenerationContext) -> Self {
        let mut draft = Self {
            context,
            document: None,
            estimated_minutes: 10,
            activities: Vec::new(),
            revision: 0,
            findings: Vec::new(),
        };
        draft.refresh_audit();
        draft
    }

    pub fn view(&self, draft_id: &str) -> LessonDraftView {
        LessonDraftView {
            draft_id: draft_id.to_owned(),
            course_title: self.context.course_title.clone(),
            module_title: self.context.module_title.clone(),
            lesson_title: self.context.lesson_title.clone(),
            purpose: self.context.purpose.clone(),
            concept_keys: self.context.concept_keys.clone(),
            grounding: match &self.context.excerpt {
                Some(excerpt) => format!("excerpt:{}", excerpt.path),
                None => "course_brief".into(),
            },
            revision: self.revision,
        }
    }

    /// Apply a batch of ops in order; each accepted op bumps the revision.
    /// Positions refer to the activity list at the moment the op applies.
    pub fn apply_ops(&mut self, ops: Vec<LessonOp>) -> LessonPatchReport {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for op in ops {
            let op_name = op.name().to_owned();
            match self.apply_one(op) {
                Ok(summary) => {
                    self.revision += 1;
                    accepted.push(summary);
                }
                Err(reason) => rejected.push(LessonRejectedOp {
                    op: op_name,
                    reason,
                }),
            }
        }
        self.refresh_audit();
        LessonPatchReport {
            revision: self.revision,
            accepted,
            rejected,
            findings: self.findings.clone(),
        }
    }

    fn apply_one(&mut self, op: LessonOp) -> Result<String, String> {
        match op {
            LessonOp::SetDocument { document } => {
                let trimmed = document.trim().to_owned();
                if trimmed.is_empty() {
                    return Err("document must not be empty".into());
                }
                let chars = trimmed.chars().filter(|c| !c.is_whitespace()).count();
                self.document = Some(trimmed);
                Ok(format!("document replaced ({chars} non-whitespace characters)"))
            }
            LessonOp::SetEstimatedMinutes { minutes } => {
                if !(1..=240).contains(&minutes) {
                    return Err(format!(
                        "estimated_minutes must be 1-240, got {minutes}"
                    ));
                }
                self.estimated_minutes = minutes;
                Ok(format!("estimated_minutes set to {minutes}"))
            }
            LessonOp::AddActivity { activity } => {
                if self.activities.len() >= MAX_ACTIVITIES {
                    return Err(format!(
                        "lesson already holds {} activities (cap {MAX_ACTIVITIES}); remove one first",
                        self.activities.len()
                    ));
                }
                let position = self.activities.len();
                self.activities.push(activity);
                Ok(format!("activity added at position {position}"))
            }
            LessonOp::UpdateActivity { position, activity } => {
                if position >= self.activities.len() {
                    return Err(format!(
                        "position {position} out of range (lesson holds {} activities, positions 0..{})",
                        self.activities.len(),
                        self.activities.len()
                    ));
                }
                self.activities[position] = activity;
                Ok(format!("activity at position {position} replaced"))
            }
            LessonOp::RemoveActivity { position } => {
                if position >= self.activities.len() {
                    return Err(format!(
                        "position {position} out of range (lesson holds {} activities, positions 0..{})",
                        self.activities.len(),
                        self.activities.len()
                    ));
                }
                self.activities.remove(position);
                Ok(format!("activity at position {position} removed"))
            }
        }
    }

    pub fn refresh_audit(&mut self) {
        let document = self.document.clone();
        let estimated_minutes = self.estimated_minutes;
        let activities = self.activities.clone();
        self.findings = audit_findings(
            &self.context,
            document.as_deref(),
            estimated_minutes,
            &activities,
        );
    }

    /// The full findings text — the repair loop's primary input.
    pub fn audit_report(&self) -> String {
        if self.findings.is_empty() {
            return "课时草稿审计：无问题 — 可以调用 ls_finish 发布。".into();
        }
        let mut report = format!("课时草稿审计 — {} 条 finding：\n", self.findings.len());
        for finding in &self.findings {
            report.push_str(&format!(
                "- [{}] {}: {}\n",
                finding.severity, finding.kind, finding.message
            ));
        }
        report.push_str(
            "\n存在 [danger] 时发布会被门禁拒绝：逐条修复（ls_set_document / ls_patch_activities）后重新 ls_audit。",
        );
        report
    }

    pub fn inspect(&self) -> LessonInspectView {
        let document = self.document.as_deref().unwrap_or_default();
        let document_sections = document
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("## "))
            .map(str::to_owned)
            .collect();
        LessonInspectView {
            revision: self.revision,
            document_written: self.document.is_some(),
            document_chars: document.chars().filter(|c| !c.is_whitespace()).count(),
            document_sections,
            estimated_minutes: self.estimated_minutes,
            activities: self
                .activities
                .iter()
                .enumerate()
                .map(|(position, activity)| LessonActivitySummary {
                    position,
                    kind: activity.kind.as_str().to_owned(),
                    prompt: activity.prompt.trim().chars().take(60).collect(),
                })
                .collect(),
            findings: self.findings.clone(),
        }
    }

    /// Convert a gate-cleared draft into the generation stage's output.
    /// Only called after the finish gate — the document is always present.
    pub fn to_output(&self) -> LessonOutput {
        LessonOutput {
            summary: self.document.clone().unwrap_or_default(),
            estimated_minutes: self.estimated_minutes,
            activities: self.activities.clone(),
        }
    }
}

/// The deterministic audit: the fallback pipeline's document + activity
/// rules, inlined with position-aware messages, plus draft-specific
/// warnings. DANGER findings block the finish gate.
fn audit_findings(
    context: &LessonGenerationContext,
    document: Option<&str>,
    estimated_minutes: i64,
    activities: &[ActivityPack],
) -> Vec<LessonFinding> {
    let mut findings = Vec::new();
    let danger = |kind: &str, message: String| LessonFinding {
        severity: SEV_DANGER,
        kind: kind.to_owned(),
        message,
    };
    let warning = |kind: &str, message: String| LessonFinding {
        severity: SEV_WARNING,
        kind: kind.to_owned(),
        message,
    };

    // ── Document contract (same validator as the fallback pipeline) ──
    match document {
        None => findings.push(danger(
            "document_missing",
            "学习文档尚未写入（ls_set_document）".into(),
        )),
        Some(document) => {
            if let Err(error) = validate_lesson_document(document) {
                findings.push(danger("document_invalid", error));
            }
        }
    }

    // ── Activity count floors / caps (validate_lesson_activities rules) ──
    if activities.len() < LESSON_MIN_ACTIVITIES {
        findings.push(danger(
            "activities_too_few",
            format!(
                "lesson has {} activities, expected at least {LESSON_MIN_ACTIVITIES}",
                activities.len()
            ),
        ));
    }
    let objective = activities
        .iter()
        .filter(|activity| activity.kind != ActivityKind::Reflection)
        .count();
    if objective < LESSON_MIN_OBJECTIVE_ACTIVITIES {
        findings.push(danger(
            "objective_activities_too_few",
            format!(
                "lesson has {objective} objective activities, expected at least {LESSON_MIN_OBJECTIVE_ACTIVITIES}"
            ),
        ));
    }
    let reflections = activities.len() - objective;
    if reflections > LESSON_MAX_REFLECTION_ACTIVITIES {
        findings.push(danger(
            "reflections_too_many",
            format!(
                "lesson has {reflections} reflection questions, expected at most {LESSON_MAX_REFLECTION_ACTIVITIES}"
            ),
        ));
    }
    if activities.len() > 5 {
        findings.push(warning(
            "activities_over_target",
            format!("lesson has {} activities; the design guidance is 3-5", activities.len()),
        ));
    }
    if reflections > 1 {
        findings.push(warning(
            "reflections_multiple",
            "prefer exactly one reflection question per lesson (more only when one question \
             cannot cover all of the lesson's concepts)"
                .into(),
        ));
    }

    // ── estimated_minutes sanity ──
    if !(5..=60).contains(&estimated_minutes) {
        findings.push(warning(
            "estimated_minutes_out_of_range",
            format!(
                "estimated_minutes is {estimated_minutes}; the design guidance is 5-60"
            ),
        ));
    }

    // ── Per-activity rules (shape + concept binding), position-aware ──
    let lesson_keys: HashSet<&str> = context
        .concept_keys
        .iter()
        .map(String::as_str)
        .collect();
    let mut seen_prompts: HashSet<String> = HashSet::new();
    for (position, activity) in activities.iter().enumerate() {
        let at = format!("activity at position {position}");
        if activity.prompt.trim().is_empty() {
            findings.push(danger("activity_prompt_empty", format!("{at}: prompt is empty")));
        }
        for concept in &activity.concepts {
            if !lesson_keys.contains(concept.as_str()) {
                findings.push(danger(
                    "concept_binding_unknown",
                    format!(
                        "{at} binds concept '{concept}' which is not bound to this lesson; \
                         bind only: {}",
                        context.concept_keys.join(", ")
                    ),
                ));
            }
        }
        match activity.kind {
            ActivityKind::SingleChoice => {
                if !(3..=5).contains(&activity.options.len()) {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: single_choice \"{}\" has {} options, expected 3-5",
                            activity.prompt,
                            activity.options.len()
                        ),
                    ));
                }
                let Some(answer) = activity.answer.as_str() else {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!("{at}: single_choice \"{}\" answer must be a string", activity.prompt),
                    ));
                    continue;
                };
                if !activity.options.iter().any(|option| option == answer) {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: single_choice \"{}\" answer does not match any option",
                            activity.prompt
                        ),
                    ));
                }
            }
            ActivityKind::TrueFalse => {
                if !activity.answer.is_boolean() {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!("{at}: true_false \"{}\" answer must be a boolean", activity.prompt),
                    ));
                }
            }
            ActivityKind::Reflection => {
                if !activity.answer.is_null() {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!("{at}: reflection \"{}\" answer must be null", activity.prompt),
                    ));
                }
            }
            ActivityKind::FillInBlank => {
                if !activity.prompt.contains("___") {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: fill_in_blank \"{}\" prompt must contain a ___ blank",
                            activity.prompt
                        ),
                    ));
                }
                let Some(answers) = activity.answer.as_array() else {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: fill_in_blank \"{}\" answer must be a JSON array of accepted answers",
                            activity.prompt
                        ),
                    ));
                    continue;
                };
                if answers.is_empty() || answers.len() > 3 {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!("{at}: fill_in_blank \"{}\" must have 1-3 accepted answers", activity.prompt),
                    ));
                }
                if answers.iter().any(|accepted| {
                    !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
                }) {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: fill_in_blank \"{}\" accepted answers must be non-empty strings",
                            activity.prompt
                        ),
                    ));
                }
                if activity
                    .distractors
                    .iter()
                    .all(|distractor| distractor.trim().is_empty())
                {
                    findings.push(danger(
                        "activity_shape_invalid",
                        format!(
                            "{at}: fill_in_blank \"{}\" must provide at least one near-synonym distractor",
                            activity.prompt
                        ),
                    ));
                }
            }
        }
        // Duplicate prompts (normalized) are redundant retrieval work.
        let normalized = activity.prompt.trim().to_lowercase();
        if !normalized.is_empty() && !seen_prompts.insert(normalized) {
            findings.push(warning(
                "activity_prompt_duplicate",
                format!("{at}: duplicates an earlier activity's prompt"),
            ));
        }
    }
    findings
}

/// The lesson-content agent engine seam: nomifun-learning owns the draft,
/// the deterministic audit and the finish gate; an injected engine (the
/// nomifun-ai-agent two-loop implementation) produces the content. Without
/// an engine, `generate_lesson_content` falls back to the legacy two-stage
/// one-shot pipeline.
#[async_trait::async_trait]
pub trait LessonContentAgentEngine: Send + Sync {
    /// Run the lesson agent loop; returns the audit-gated lesson output.
    async fn generate(
        &self,
        user_id: &nomifun_common::UserId,
        context: &LessonGenerationContext,
        model_override: Option<(&str, &str)>,
    ) -> Result<LessonOutput, nomifun_common::AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> LessonGenerationContext {
        LessonGenerationContext {
            course_title: "测试课程".into(),
            course_description: "零基础期权入门".into(),
            module_title: "模块一".into(),
            module_index: 0,
            lesson_title: "课时一".into(),
            lesson_index: 0,
            total_lessons: 2,
            next_lesson_title: Some("课时二".into()),
            purpose: "理解期权的定义".into(),
            concepts: vec![ConceptPack {
                key: "c1".into(),
                title: "期权定义".into(),
                description: "权利与义务的不对称".into(),
                prerequisites: Vec::new(),
            }],
            concept_keys: vec!["c1".into()],
            excerpt: Some(LessonExcerpt {
                path: "docs/basics.md".into(),
                text: "期权的定义……".into(),
            }),
            outline_tree: String::new(),
            adjacent_context: String::new(),
            graph: None,
        }
    }

    /// ≥800 non-whitespace characters across the three required sections
    /// (10 × 30-char sentence per section = 912 including the headings).
    fn long_document() -> String {
        let body = "这是一个用于测试的完整段落，覆盖课时要求的知识点并且足够长。".repeat(10);
        format!("## 描述\n{body}\n## 例子\n{body}\n## 验证\n{body}\n")
    }

    fn activity_json(kind: &str, extra: serde_json::Value) -> ActivityPack {
        let mut value = serde_json::json!({
            "kind": kind,
            "prompt": "期权的本质是什么？",
            "explanation": "因为买方持有权利。",
            "concepts": ["c1"]
        });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            object.extend(extra.clone());
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn empty_draft_audits_document_and_activity_dangers() {
        let draft = LessonDraft::new(context());
        let kinds: Vec<&str> = draft.findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"document_missing"));
        assert!(kinds.contains(&"activities_too_few"));
        assert!(kinds.contains(&"objective_activities_too_few"));
        assert!(draft.audit_report().contains("[danger]"));
    }

    #[test]
    fn gate_rules_mirror_validate_lesson_contract() {
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: long_document(),
        }]);
        // Document only: activities dangers remain, document dangers gone.
        let kinds: Vec<&str> = draft.findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(!kinds.contains(&"document_missing"));
        assert!(!kinds.contains(&"document_invalid"));
        assert!(kinds.contains(&"activities_too_few"));

        // A short document fails the length floor through the same validator.
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: "## 描述\n太短\n## 例子\n还是短\n## 验证\n依然短".into(),
        }]);
        assert!(draft
            .findings
            .iter()
            .any(|f| f.kind == "document_invalid"
                && f.message.contains("non-whitespace characters")));

        // Unknown concept binding is rejected with the lesson's keys named.
        draft.apply_ops(vec![LessonOp::SetDocument {
            document: long_document(),
        }]);
        draft.apply_ops(vec![
            LessonOp::AddActivity {
                activity: activity_json("single_choice", serde_json::json!({
                    "options": ["权利", "义务", "债务"], "answer": "权利"
                })),
            },
            LessonOp::AddActivity {
                activity: activity_json("true_false", serde_json::json!({ "answer": true })),
            },
            LessonOp::AddActivity {
                activity: ActivityPack {
                    concepts: vec!["ghost".into()],
                    ..activity_json("reflection", serde_json::json!({ "answer": null }))
                },
            },
        ]);
        let unknown = draft
            .findings
            .iter()
            .find(|f| f.kind == "concept_binding_unknown")
            .expect("ghost binding must be flagged");
        assert!(unknown.message.contains("ghost"));
        assert!(unknown.message.contains("c1"), "names the allowed keys");
        // Objective floor is now satisfied, so only the binding blocks.
        assert!(!draft
            .findings
            .iter()
            .any(|f| f.kind == "objective_activities_too_few"));
    }

    #[test]
    fn finish_gate_converts_a_clean_draft_into_lesson_output() {
        let mut draft = LessonDraft::new(context());
        draft.apply_ops(vec![
            LessonOp::SetDocument {
                document: long_document(),
            },
            LessonOp::SetEstimatedMinutes { minutes: 15 },
            LessonOp::AddActivity {
                activity: activity_json("single_choice", serde_json::json!({
                    "options": ["权利", "义务", "债务"], "answer": "权利"
                })),
            },
            LessonOp::AddActivity {
                activity: activity_json("true_false", serde_json::json!({ "answer": false })),
            },
            LessonOp::AddActivity {
                activity: activity_json("reflection", serde_json::json!({ "answer": null })),
            },
        ]);
        assert!(draft.findings.iter().all(|f| f.severity != SEV_DANGER));
        let output = draft.to_output();
        assert_eq!(output.estimated_minutes, 15);
        assert_eq!(output.activities.len(), 3);
        assert!(output.summary.contains("## 描述"));
        // The gate itself lives on the service facade; here only the shape.
        assert_eq!(draft.view("d1").grounding, "excerpt:docs/basics.md");
    }

    #[test]
    fn rejected_ops_do_not_bump_the_revision() {
        let mut draft = LessonDraft::new(context());
        let report = draft.apply_ops(vec![
            LessonOp::RemoveActivity { position: 3 },
            LessonOp::SetEstimatedMinutes { minutes: 0 },
        ]);
        assert_eq!(report.accepted.len(), 0);
        assert_eq!(report.rejected.len(), 2);
        assert_eq!(report.revision, 0, "no accepted op, no revision bump");
        assert_eq!(report.rejected[0].op, "remove_activity");
        assert!(report.rejected[1].reason.contains("1-240"));

        let report = draft.apply_ops(vec![LessonOp::SetEstimatedMinutes { minutes: 20 }]);
        assert_eq!(report.revision, 1);
        assert_eq!(draft.estimated_minutes, 20);
    }
}
