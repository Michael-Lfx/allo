//! The course outline draft the `co_*` agent tools edit. Stable-keyed
//! entities (modules, lessons), a batched op vocabulary with per-op
//! rejection reasons, a live deterministic audit, and a single audit-gated
//! publish path that converts the draft into the generation stage's
//! [`Blueprint`].
//!
//! Identity model: every module/lesson carries a model-chosen key (unique
//! per collection), so patch ops stay unambiguous while the draft is
//! re-ordered or renamed. Ops validate references eagerly — an unknown key
//! is rejected with the closest existing candidate — while the audit
//! re-checks everything deterministically on every patch and at the finish
//! gate (DANGER findings block publishing). The per-course concept system
//! is retired (ADR-0009): the outline carries no concepts and no
//! prerequisite edges.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::learning_graph::ScopeAnalysis;
use crate::generation::{Blueprint, BlueprintLesson, BlueprintModule};
use crate::models::SourceSpan;

use super::{
    AuditFinding, BLOCK_MIN_SHARED, OutlineBrief, SEV_DANGER, SEV_INFO, SEV_WARNING,
    common_substring_len,
};

/// Hard caps so a runaway agent loop cannot inflate the draft without
/// bound; both leave generous headroom above any course shape the model
/// may reasonably pick for itself.
const MAX_MODULES: usize = 12;
const MAX_LESSONS_PER_MODULE: usize = 12;

// ── Draft entities ─────────────────────────────────────────────────────────

/// One course module: keyed, titled, and owning an ordered lesson list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineModule {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Ordered lesson keys of this module.
    #[serde(default)]
    pub lessons: Vec<String>,
}

/// One lesson: title/purpose plus, on the kb flow, the sampled file it
/// grounds in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineLesson {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub purpose: String,
    /// kb flow only: exact sampled file path. `None` on the description
    /// flow (no samples exist) and dropped on import.
    #[serde(default)]
    pub source: Option<String>,
}

/// Course-level title/description, set once via `set_meta`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutlineMeta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
}

// ── The op vocabulary ──────────────────────────────────────────────────────

/// One batched edit on the draft outline. Serialized as a tagged JSON
/// object (`{"op": "add_module", ...}`). Reference fields must name keys
/// that already exist (or are created by an earlier op of the same batch);
/// violations are rejected per-op with a closest-candidate hint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OutlineOp {
    /// Set the course-level title (required) and description.
    SetMeta {
        title: String,
        #[serde(default)]
        description: Option<String>,
    },
    /// Insert a new module. Keys must be unique across modules.
    AddModule {
        key: String,
        title: String,
        #[serde(default)]
        description: Option<String>,
    },
    /// Rename / re-describe a module.
    UpdateModule {
        key: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        description: Option<String>,
    },
    /// Remove a module and every lesson inside it.
    RemoveModule { key: String },
    /// Append a lesson to a module.
    AddLesson {
        module: String,
        key: String,
        title: String,
        #[serde(default)]
        purpose: Option<String>,
        /// kb flow: exact sampled file path (must be one of the sample paths).
        #[serde(default)]
        source: Option<String>,
    },
    /// Update an existing lesson. The source follows double-option
    /// semantics: omitted = untouched, `null` = cleared, a path = set.
    UpdateLesson {
        key: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        purpose: Option<String>,
        #[serde(default, deserialize_with = "de_double_option")]
        source: Option<Option<String>>,
    },
    /// Remove a lesson from its module.
    RemoveLesson { key: String },
}

/// Distinguish an explicit `"source": null` (clear the source) from an
/// omitted field (leave it untouched): a missing field still deserializes
/// to the default `None`, while `null` wraps into `Some(None)` — the shape
/// `op_update_lesson` expects.
fn de_double_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

// ── The draft ──────────────────────────────────────────────────────────────

/// An outline draft under construction. Held in memory by the service
/// layer (generation is short-lived; drafts do not survive restarts).
#[derive(Debug, Clone)]
pub struct OutlineDraft {
    /// The generation brief the draft is being built against.
    pub brief: OutlineBrief,
    /// kb flow: sampled `(path, excerpt)` pairs; `co_read` opens them by
    /// exact path. Empty on the description flow.
    pub samples: Vec<(String, String)>,
    /// Best-effort scope analysis (may be absent when the scope call
    /// failed); its blocks become the audit's coverage contract.
    pub scope: Option<ScopeAnalysis>,
    pub meta: OutlineMeta,
    pub modules: Vec<OutlineModule>,
    pub lessons: Vec<OutlineLesson>,
    /// Increments once per accepted op, so tool callers can detect
    /// concurrent edits.
    pub revision: u64,
    /// Cached audit snapshot; refreshed after every patch and re-run live
    /// by the finish gate.
    pub findings: Vec<AuditFinding>,
}

impl OutlineDraft {
    pub(crate) fn new(
        brief: OutlineBrief,
        samples: Vec<(String, String)>,
        scope: Option<ScopeAnalysis>,
    ) -> Self {
        let mut draft = Self {
            brief,
            samples,
            scope,
            meta: OutlineMeta::default(),
            modules: Vec::new(),
            lessons: Vec::new(),
            revision: 0,
            findings: Vec::new(),
        };
        // A fresh draft is ALREADY audited: the size mismatch carries from
        // birth, so a premature `co_finish` on a never-patched draft is
        // rejected instead of publishing a blank course.
        draft.refresh_audit();
        draft
    }

    /// Apply a batch of operations in order and re-run the audit gate.
    /// Accepted ops mutate the draft and bump `revision`; rejected ops are
    /// reported with a reason and leave the draft untouched.
    pub(crate) fn apply_ops(&mut self, ops: Vec<OutlineOp>) -> OutlinePatchReport {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for op in ops {
            match self.apply_one(&op) {
                Ok(outcome) => accepted.push(outcome),
                Err(reason) => rejected.push(RejectedOutlineOp { reason }),
            }
        }
        self.refresh_audit();
        OutlinePatchReport {
            revision: self.revision,
            accepted,
            rejected,
            findings: summarize_findings(&self.findings),
        }
    }

    fn apply_one(&mut self, op: &OutlineOp) -> Result<OpOutcome, String> {
        match op {
            OutlineOp::SetMeta { title, description } => {
                self.op_set_meta(title, description.as_deref())
            }
            OutlineOp::AddModule { key, title, description } => {
                self.op_add_module(key, title, description.as_deref())
            }
            OutlineOp::UpdateModule { key, title, description } => {
                self.op_update_module(key, title.as_deref(), description.as_deref())
            }
            OutlineOp::RemoveModule { key } => self.op_remove_module(key),
            OutlineOp::AddLesson { module, key, title, purpose, source } => self
                .op_add_lesson(
                    module,
                    key,
                    title,
                    purpose.as_deref(),
                    source.as_deref(),
                ),
            OutlineOp::UpdateLesson { key, title, purpose, source } => self
                .op_update_lesson(key, title.as_deref(), purpose.as_deref(), source),
            OutlineOp::RemoveLesson { key } => self.op_remove_lesson(key),
        }
    }

    fn op_set_meta(&mut self, title: &str, description: Option<&str>) -> Result<OpOutcome, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("set_meta: course title must not be empty".into());
        }
        self.meta.title = title.to_owned();
        if let Some(description) = description {
            self.meta.description = description.trim().to_owned();
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "set_meta".into(),
            summary: format!("已设置课程标题「{title}」"),
        })
    }

    fn op_add_module(
        &mut self,
        key: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<OpOutcome, String> {
        let key = normalize_key(key, "add_module")?;
        let title = require_title(title, "add_module")?;
        if self.modules.iter().any(|module| module.key == key) {
            return Err(format!("add_module: module key '{key}' already exists"));
        }
        if self.modules.len() >= MAX_MODULES {
            return Err(format!(
                "add_module: too many modules (cap {MAX_MODULES}); remove one first"
            ));
        }
        self.modules.push(OutlineModule {
            key: key.clone(),
            title: title.to_owned(),
            description: description.unwrap_or_default().trim().to_owned(),
            lessons: Vec::new(),
        });
        self.revision += 1;
        Ok(OpOutcome {
            op: "add_module".into(),
            summary: format!("已添加模块 '{key}'（{title}）"),
        })
    }

    fn op_update_module(
        &mut self,
        key: &str,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<OpOutcome, String> {
        let key = key.trim();
        let module = self
            .modules
            .iter_mut()
            .find(|module| module.key == key)
            .ok_or_else(|| format!("update_module: unknown module key '{key}'"))?;
        if let Some(title) = title {
            module.title = require_title(title, "update_module")?.to_owned();
        }
        if let Some(description) = description {
            module.description = description.trim().to_owned();
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "update_module".into(),
            summary: format!("已更新模块 '{key}'"),
        })
    }

    fn op_remove_module(&mut self, key: &str) -> Result<OpOutcome, String> {
        let key = key.trim();
        let position = self
            .modules
            .iter()
            .position(|module| module.key == key)
            .ok_or_else(|| format!("remove_module: unknown module key '{key}'"))?;
        let removed = self.modules.remove(position);
        let removed_lessons = removed.lessons.len();
        self.lessons
            .retain(|lesson| !removed.lessons.contains(&lesson.key));
        self.revision += 1;
        Ok(OpOutcome {
            op: "remove_module".into(),
            summary: format!(
                "已移除模块 '{key}'（连带 {} 个课时）",
                removed_lessons
            ),
        })
    }

    fn op_add_lesson(
        &mut self,
        module: &str,
        key: &str,
        title: &str,
        purpose: Option<&str>,
        source: Option<&str>,
    ) -> Result<OpOutcome, String> {
        let module_key = module.trim();
        let module_position = self
            .modules
            .iter()
            .position(|candidate| candidate.key == module_key)
            .ok_or_else(|| format!("add_lesson: unknown module key '{module_key}'"))?;
        let key = normalize_key(key, "add_lesson")?;
        let title = require_title(title, "add_lesson")?;
        if self.lessons.iter().any(|lesson| lesson.key == key) {
            return Err(format!("add_lesson: lesson key '{key}' already exists"));
        }
        if self.modules[module_position].lessons.len() >= MAX_LESSONS_PER_MODULE {
            return Err(format!(
                "add_lesson: module '{module_key}' has too many lessons (cap {MAX_LESSONS_PER_MODULE})"
            ));
        }
        let source = self.resolve_source(source, "add_lesson")?;
        let purpose = purpose.unwrap_or_default().trim().to_owned();
        if purpose.is_empty() {
            return Err("add_lesson: purpose is required (what the learner can do afterwards)".into());
        }
        self.modules[module_position].lessons.push(key.clone());
        self.lessons.push(OutlineLesson {
            key: key.clone(),
            title: title.to_owned(),
            purpose,
            source,
        });
        self.revision += 1;
        Ok(OpOutcome {
            op: "add_lesson".into(),
            summary: format!("已添加课时 '{key}'（{title}）到模块 '{module_key}'"),
        })
    }

    fn op_update_lesson(
        &mut self,
        key: &str,
        title: Option<&str>,
        purpose: Option<&str>,
        source: &Option<Option<String>>,
    ) -> Result<OpOutcome, String> {
        let key = key.trim();
        // Resolve the source reference against the whole draft first — the
        // mutable lesson borrow must not overlap the lookup.
        let resolved_source = match source {
            Some(value) => Some(self.resolve_source(value.as_deref(), "update_lesson")?),
            None => None,
        };
        let lesson = self
            .lessons
            .iter_mut()
            .find(|lesson| lesson.key == key)
            .ok_or_else(|| format!("update_lesson: unknown lesson key '{key}'"))?;
        if let Some(title) = title {
            lesson.title = require_title(title, "update_lesson")?.to_owned();
        }
        if let Some(purpose) = purpose {
            lesson.purpose = purpose.trim().to_owned();
        }
        if let Some(resolved) = resolved_source {
            lesson.source = resolved;
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "update_lesson".into(),
            summary: format!("已更新课时 '{key}'"),
        })
    }

    fn op_remove_lesson(&mut self, key: &str) -> Result<OpOutcome, String> {
        let key = key.trim();
        let position = self
            .lessons
            .iter()
            .position(|lesson| lesson.key == key)
            .ok_or_else(|| format!("remove_lesson: unknown lesson key '{key}'"))?;
        self.lessons.remove(position);
        for module in &mut self.modules {
            module.lessons.retain(|lesson| lesson != key);
        }
        self.revision += 1;
        Ok(OpOutcome {
            op: "remove_lesson".into(),
            summary: format!("已移除课时 '{key}'"),
        })
    }

    // ── Reference resolution (reject with a closest-candidate hint) ─────

    fn resolve_source(&self, source: Option<&str>, op: &str) -> Result<Option<String>, String> {
        let Some(path) = source else {
            return Ok(None);
        };
        let path = path.trim();
        if path.is_empty() {
            return Ok(None);
        }
        if self.samples.is_empty() {
            return Err(format!(
                "{op}: this generation has no sampled documents, so lessons cannot cite a source"
            ));
        }
        if self.samples.iter().any(|(candidate, _)| candidate == path) {
            return Ok(Some(path.to_owned()));
        }
        let closest = self
            .samples
            .iter()
            .map(|(candidate, _)| candidate.as_str())
            .min_by_key(|candidate| common_substring_len(path, candidate))
            .unwrap_or("");
        Err(format!(
            "{op}: '{path}' is not a sampled file; exact paths are like '{closest}'"
        ))
    }

    /// kb flow: read one sampled file by exact path (the `co_read` tool).
    pub(crate) fn read_sample(&self, path: &str) -> Option<&str> {
        self.samples
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, excerpt)| excerpt.as_str())
    }

    /// Sample paths available to `co_read` (empty on the description flow).
    pub(crate) fn sample_paths(&self) -> Vec<String> {
        self.samples.iter().map(|(path, _)| path.clone()).collect()
    }
}

/// Trim + validate an entity key: non-empty, no whitespace-only value.
fn normalize_key(key: &str, op: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("{op}: key must not be empty"));
    }
    if key.chars().count() > 80 {
        return Err(format!("{op}: key '{key}' is too long (max 80 chars)"));
    }
    Ok(key.to_owned())
}

fn require_title<'a>(title: &'a str, op: &str) -> Result<&'a str, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(format!("{op}: title must not be empty"));
    }
    Ok(title)
}

/// How many danger findings the audit report lists in full before
/// collapsing the rest into a trailing count (mirrors the event payload's
/// top-findings cap).
pub const AUDIT_REPORT_DANGER_LIMIT: usize = 5;

// ── Deterministic audit ────────────────────────────────────────────────────

impl OutlineDraft {
    /// Re-run every deterministic check and cache the findings. Called
    /// after every patch and again live by the finish gate — the gate
    /// never trusts a cached snapshot.
    pub(crate) fn refresh_audit(&mut self) {
        self.findings = audit_outline(self);
    }

    /// The agent-facing report: the scope checklists with live coverage
    /// plus every finding (danger findings first). The repair loop's
    /// primary input, same shape as the concept graph's report.
    pub(crate) fn audit_report(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if let Some(scope) = &self.scope {
            lines.push("==== Scope reference ====".into());
            if !scope.scope.is_empty() {
                lines.push(format!("范围界定：{}", scope.scope));
            }
            if !scope.blocks.is_empty() {
                let missing: Vec<&String> = scope
                    .blocks
                    .iter()
                    .filter(|block| !self.block_covered(block))
                    .collect();
                lines.push(format!(
                    "大块概念（{} 个，{} 个已覆盖）",
                    scope.blocks.len(),
                    scope.blocks.len() - missing.len()
                ));
                for block in &missing {
                    lines.push(format!("- 未覆盖：{block}"));
                }
            }
        }
        lines.push("==== Audit report ====".into());
        lines.push(format!(
            "{} modules / {} lessons",
            self.modules.len(),
            self.lessons.len()
        ));
        if self.findings.is_empty() {
            lines.push("No findings. Ready to finish.".into());
            return lines.join("\n");
        }
        let mut danger = 0usize;
        for finding in &self.findings {
            if finding.severity == SEV_DANGER {
                if danger < AUDIT_REPORT_DANGER_LIMIT {
                    lines.push(format!("[danger] {}", finding.message));
                }
                danger += 1;
            } else {
                lines.push(format!("[{}] {}", finding.severity, finding.message));
            }
        }
        if danger > AUDIT_REPORT_DANGER_LIMIT {
            lines.push(format!(
                "... and {} more danger findings",
                danger - AUDIT_REPORT_DANGER_LIMIT
            ));
        }
        lines.push("Fix every danger finding, then call co_finish again.".into());
        lines.join("\n")
    }

    /// A scope block counts as covered when some module or lesson title
    /// shares a meaningful substring with it (the deliberately weak bar of
    /// [`BLOCK_MIN_SHARED`]). The bar is capped by the block's own
    /// character length: the scope analysis can legitimately emit
    /// single-character blocks ("栈", "图") whose longest possible shared
    /// substring is 1 — without the cap such blocks are unmatchable and the
    /// audit deadlocks the whole run (the model patches forever guessing at
    /// the rule).
    fn block_covered(&self, block: &str) -> bool {
        let bar = scope_block_bar(block);
        let covered = |text: &str| common_substring_len(text, block) >= bar;
        self.modules.iter().any(|module| covered(&module.title))
            || self.lessons.iter().any(|lesson| covered(&lesson.title))
    }

    /// Convert a draft that passed the finish gate into the generation
    /// stage's blueprint. DANGER-free by construction of the gate; no
    /// validation runs here — `import_course_outline` re-validates the pack.
    pub(crate) fn to_blueprint(&self) -> Blueprint {
        Blueprint {
            title: self.meta.title.trim().to_owned(),
            description: self.meta.description.trim().to_owned(),
            domain: self.brief.domain.clone().unwrap_or_default(),
            version: 1,
            modules: self
                .modules
                .iter()
                .map(|module| BlueprintModule {
                    title: module.title.clone(),
                    description: module.description.clone(),
                    lessons: module
                        .lessons
                        .iter()
                        .filter_map(|key| self.lessons.iter().find(|lesson| &lesson.key == key))
                        .map(|lesson| BlueprintLesson {
                            title: lesson.title.clone(),
                            purpose: lesson.purpose.clone(),
                            source: lesson.source.as_ref().map(|path| SourceSpan {
                                path: path.clone(),
                                start: None,
                                end: None,
                            }),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

/// Run every deterministic structural check over the draft.
fn audit_outline(draft: &OutlineDraft) -> Vec<AuditFinding> {
    let mut findings = Vec::new();

    if draft.meta.title.trim().is_empty() {
        findings.push(finding(
            "meta_missing",
            SEV_DANGER,
            "course title is not set (set_meta first)".into(),
            vec![],
        ));
    }
    // Structural minimum only — the course size itself is the model's call.
    // Without these the finish gate would let an empty outline through and
    // the import would blow up on a blueprint with no teachable content.
    if draft.modules.is_empty() {
        findings.push(finding(
            "no_modules",
            SEV_DANGER,
            "outline has no modules (add_module first)".into(),
            vec![],
        ));
    }
    for module in &draft.modules {
        if module.lessons.is_empty() {
            findings.push(finding(
                "empty_module",
                SEV_DANGER,
                format!("module '{}' carries no lessons (add_lesson first)", module.key),
                vec![module.key.clone()],
            ));
        }
    }

    push_duplicates(
        &mut findings,
        "module",
        draft.modules.iter().map(|module| module.key.clone()),
    );
    push_duplicates(
        &mut findings,
        "lesson",
        draft.lessons.iter().map(|lesson| lesson.key.clone()),
    );
    push_duplicate_titles(
        &mut findings,
        draft.lessons.iter().map(|lesson| lesson.title.clone()),
    );

    for lesson in &draft.lessons {
        if lesson.title.trim().is_empty() {
            findings.push(finding(
                "lesson_missing_field",
                SEV_DANGER,
                format!("lesson '{}' has no title", lesson.key),
                vec![lesson.key.clone()],
            ));
        }
        if lesson.purpose.trim().is_empty() {
            findings.push(finding(
                "lesson_missing_field",
                SEV_DANGER,
                format!("lesson '{}' has no purpose", lesson.key),
                vec![lesson.key.clone()],
            ));
        }
        if let Some(source) = &lesson.source {
            if !draft.samples.iter().any(|(path, _)| path == source) {
                findings.push(finding(
                    "unknown_source",
                    SEV_DANGER,
                    format!("lesson '{}' cites unsampled source '{source}'", lesson.key),
                    vec![lesson.key.clone()],
                ));
            }
        }
    }

    for key in draft
        .modules
        .iter()
        .map(|module| module.key.as_str())
        .chain(draft.lessons.iter().map(|lesson| lesson.key.as_str()))
    {
        if !kebabish(key) {
            findings.push(finding(
                "naming_style",
                SEV_INFO,
                format!("key '{key}' is not lowercase-kebab; prefer short latin keys"),
                vec![],
            ));
        }
    }

    if let Some(scope) = &draft.scope {
        for block in &scope.blocks {
            if !draft.block_covered(block) {
                let bar = scope_block_bar(block);
                findings.push(finding(
                    "scope_gap",
                    SEV_DANGER,
                    format!(
                        "scope block '{block}' is not covered by any module or lesson title \
                         (coverage = a title sharing at least {bar} consecutive characters with the \
                         block — a title containing '{block}' always counts); fix: add or rename one \
                         module/lesson whose title contains '{block}'"
                    ),
                    vec![],
                ));
            }
        }
    }

    findings
}

/// Coverage bar for one scope block: `BLOCK_MIN_SHARED` shared consecutive
/// characters, capped by the block's own length (min 1) — a single-character
/// block is covered by exact title containment, longer blocks keep the
/// deliberately weak 2-char bar.
fn scope_block_bar(block: &str) -> usize {
    BLOCK_MIN_SHARED.min(block.chars().count()).max(1)
}

fn finding(kind: &str, severity: &str, message: String, evidence: Vec<String>) -> AuditFinding {
    AuditFinding {
        kind: kind.to_owned(),
        severity: severity.to_owned(),
        message,
        node_ids: evidence,
    }
}

fn push_duplicates(
    findings: &mut Vec<AuditFinding>,
    label: &str,
    keys: impl Iterator<Item = String>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates: Vec<String> = Vec::new();
    for key in keys {
        let key = key.trim().to_owned();
        if !seen.insert(key.clone()) && !duplicates.contains(&key) {
            duplicates.push(key);
        }
    }
    for key in duplicates {
        findings.push(finding(
            "duplicate_key",
            SEV_DANGER,
            format!("duplicate {label} key: {key}"),
            vec![key],
        ));
    }
}

fn push_duplicate_titles(
    findings: &mut Vec<AuditFinding>,
    titles: impl Iterator<Item = String>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut duplicates: Vec<String> = Vec::new();
    for title in titles {
        let title = title.trim().to_owned();
        if title.is_empty() {
            continue;
        }
        if !seen.insert(title.clone()) && !duplicates.contains(&title) {
            duplicates.push(title);
        }
    }
    for title in duplicates {
        findings.push(finding(
            "duplicate_title",
            SEV_WARNING,
            format!("duplicate lesson title: {title}"),
            vec![],
        ));
    }
}

/// Lowercase-kebab check for the info-grade naming hint.
fn kebabish(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn summarize_findings(findings: &[AuditFinding]) -> Vec<OutlineFindingSummary> {
    let mut order: Vec<(&str, &str)> = Vec::new();
    let mut counts: HashMap<(&str, &str), usize> = HashMap::new();
    let mut examples: HashMap<(&str, &str), Vec<String>> = HashMap::new();
    for finding in findings {
        let key = (finding.severity.as_str(), finding.kind.as_str());
        if !counts.contains_key(&key) {
            order.push(key);
        }
        *counts.entry(key).or_insert(0) += 1;
        let slot = examples.entry(key).or_default();
        if slot.len() < 3 {
            slot.push(finding.message.clone());
        }
    }
    order
        .into_iter()
        .map(|(severity, kind)| OutlineFindingSummary {
            severity: severity.to_owned(),
            kind: kind.to_owned(),
            count: counts[&(severity, kind)],
            examples: examples.remove(&(severity, kind)).unwrap_or_default(),
        })
        .collect()
}

// ── Views returned to the agent tools ──────────────────────────────────────

/// Creation view of a draft (returned by `co_start`).
#[derive(Debug, Clone, Serialize)]
pub struct OutlineDraftView {
    pub draft_id: String,
    /// `description` or `knowledge_base`.
    pub kind: String,
    /// kb name or the description head — log/event context only.
    pub topic_hint: String,
    /// kb flow: the sample paths `co_read` can open.
    pub sample_paths: Vec<String>,
    pub scope: Option<ScopeSummary>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeSummary {
    pub scope: String,
    pub blocks: Vec<String>,
}

/// One accepted op.
#[derive(Debug, Clone, Serialize)]
pub struct OpOutcome {
    pub op: String,
    pub summary: String,
}

/// One rejected op with its reason.
#[derive(Debug, Clone, Serialize)]
pub struct RejectedOutlineOp {
    pub reason: String,
}

/// Severity/kind-grouped audit counts with a few example messages.
#[derive(Debug, Clone, Serialize)]
pub struct OutlineFindingSummary {
    pub severity: String,
    pub kind: String,
    pub count: usize,
    pub examples: Vec<String>,
}

/// Result of one `co_patch` batch: per-op verdicts plus the fresh audit
/// snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct OutlinePatchReport {
    pub revision: u64,
    pub accepted: Vec<OpOutcome>,
    pub rejected: Vec<RejectedOutlineOp>,
    pub findings: Vec<OutlineFindingSummary>,
}

impl OutlinePatchReport {
    /// Total danger findings in the fresh snapshot — the loop's continue
    /// signal for the repair rounds.
    pub fn danger_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|summary| summary.severity == SEV_DANGER)
            .map(|summary| summary.count)
            .sum()
    }
}

/// Overview returned by `co_inspect`.
#[derive(Debug, Clone, Serialize)]
pub struct OutlineInspectView {
    pub revision: u64,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub modules: usize,
    pub lessons: usize,
    /// One line per module with its lesson keys.
    pub module_lines: Vec<String>,
    pub findings: Vec<OutlineFindingSummary>,
    pub sample_paths: Vec<String>,
}

/// Filter for `co_query` (substring match, default limit 50).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OutlineQuery {
    #[serde(default)]
    pub filter: String,
    #[serde(default = "default_query_limit")]
    pub limit: usize,
}

const fn default_query_limit() -> usize {
    50
}

/// Filtered lists returned by `co_query`.
#[derive(Debug, Clone, Serialize)]
pub struct OutlineQueryView {
    pub modules: Vec<String>,
    pub lessons: Vec<String>,
}

impl OutlineDraft {
    pub(crate) fn view(&self, draft_id: &str) -> OutlineDraftView {
        let topic_hint = match (&self.brief.knowledge_base, &self.brief.description) {
            (Some(kb), _) => kb.name.clone(),
            (_, Some(description)) => description.chars().take(40).collect(),
            _ => String::new(),
        };
        OutlineDraftView {
            draft_id: draft_id.to_owned(),
            kind: self.brief.kind().to_owned(),
            topic_hint,
            sample_paths: self.sample_paths(),
            scope: self.scope.as_ref().map(|scope| ScopeSummary {
                scope: scope.scope.clone(),
                blocks: scope.blocks.clone(),
            }),
            revision: self.revision,
        }
    }

    pub(crate) fn inspect(&self) -> OutlineInspectView {
        let module_lines = self
            .modules
            .iter()
            .map(|module| {
                format!(
                    "{}「{}」({} 课时: {})",
                    module.key,
                    module.title,
                    module.lessons.len(),
                    module.lessons.join(", ")
                )
            })
            .collect();
        OutlineInspectView {
            revision: self.revision,
            kind: self.brief.kind().to_owned(),
            title: self.meta.title.clone(),
            description: self.meta.description.clone(),
            modules: self.modules.len(),
            lessons: self.lessons.len(),
            module_lines,
            findings: summarize_findings(&self.findings),
            sample_paths: self.sample_paths(),
        }
    }

    pub(crate) fn query(&self, filter: &OutlineQuery) -> OutlineQueryView {
        let needle = filter.filter.trim();
        let matches = |text: &str| needle.is_empty() || text.contains(needle);
        let cap = filter.limit.max(1);
        OutlineQueryView {
            modules: self
                .modules
                .iter()
                .filter(|module| matches(&module.title) || matches(&module.key))
                .take(cap)
                .map(|module| format!("{}「{}」", module.key, module.title))
                .collect(),
            lessons: self
                .lessons
                .iter()
                .filter(|lesson| matches(&lesson.title) || matches(&lesson.key))
                .take(cap)
                .map(|lesson| {
                    let source = lesson
                        .source
                        .as_deref()
                        .map(|path| format!("; source: {path}"))
                        .unwrap_or_default();
                    format!(
                        "{}「{}」— {}{source}",
                        lesson.key,
                        lesson.title,
                        lesson.purpose
                    )
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course_outline::KnowledgeBaseBrief;

    fn kb_brief() -> OutlineBrief {
        OutlineBrief {
            description: None,
            knowledge_base: Some(KnowledgeBaseBrief {
                kb_id: "0190f5fe-7c00-7a00-8abc-000000000001".into(),
                name: "数学基础".into(),
                description: "代数入门".into(),
            }),
            samples: Vec::new(),
            domain: Some("math".into()),
        }
    }

    fn description_brief() -> OutlineBrief {
        OutlineBrief {
            description: Some("向量代数入门：从坐标到线性组合".into()),
            knowledge_base: None,
            samples: Vec::new(),
            domain: Some("math".into()),
        }
    }

    fn samples() -> Vec<(String, String)> {
        vec![("notes/vectors.md".into(), "# 向量\n具有大小和方向的量".into())]
    }

    fn op(json: &str) -> OutlineOp {
        serde_json::from_str(json).expect("op JSON should parse")
    }

    fn danger_kinds(draft: &OutlineDraft) -> Vec<&str> {
        draft
            .findings
            .iter()
            .filter(|finding| finding.severity == SEV_DANGER)
            .map(|finding| finding.kind.as_str())
            .collect()
    }

    /// A minimal complete draft: 1 module × 1 lesson.
    fn build_complete_draft(brief: OutlineBrief) -> OutlineDraft {
        let kb = brief.knowledge_base.is_some();
        let mut draft = OutlineDraft::new(brief, if kb { samples() } else { Vec::new() }, None);
        let mut ops = vec![
            op(r#"{"op":"set_meta","title":"向量代数","description":"一节课弄懂向量"}"#),
            op(r#"{"op":"add_module","key":"m1","title":"向量基础"}"#),
        ];
        if kb {
            ops.push(op(
                r#"{"op":"add_lesson","module":"m1","key":"l1","title":"什么是向量","purpose":"能用坐标表示向量","source":"notes/vectors.md"}"#,
            ));
        } else {
            ops.push(op(
                r#"{"op":"add_lesson","module":"m1","key":"l1","title":"什么是向量","purpose":"能用坐标表示向量"}"#,
            ));
        }
        let report = draft.apply_ops(ops);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        draft
    }

    #[test]
    fn complete_draft_passes_audit_and_converts_to_blueprint() {
        let mut draft = build_complete_draft(kb_brief());
        draft.refresh_audit();
        let dangers = danger_kinds(&draft);
        assert!(dangers.is_empty(), "{dangers:?}");
        let blueprint = draft.to_blueprint();
        assert_eq!(blueprint.title, "向量代数");
        assert_eq!(blueprint.domain, "math");
        assert_eq!(blueprint.modules.len(), 1);
        assert_eq!(blueprint.modules[0].lessons.len(), 1);
        assert_eq!(
            blueprint.modules[0].lessons[0]
                .source
                .as_ref()
                .unwrap()
                .path,
            "notes/vectors.md"
        );
    }

    #[test]
    fn fresh_draft_carries_danger_from_birth() {
        let draft = OutlineDraft::new(kb_brief(), samples(), None);
        let kinds = danger_kinds(&draft);
        assert!(kinds.contains(&"no_modules"));
        assert!(kinds.contains(&"meta_missing"));
        // The fresh-draft audit is what a premature finish gate rejects on.
        assert!(
            draft.audit_report().contains("[danger]"),
            "report should list danger findings"
        );
    }

    #[test]
    fn unknown_lesson_reference_names_the_module_mismatch() {
        // 概念引用随概念体系退役；引用校验现在只覆盖模块键——未知模块的
        // 课时添加被拒绝并点名未知键。
        let mut draft = OutlineDraft::new(kb_brief(), samples(), None);
        let report = draft.apply_ops(vec![
            op(r#"{"op":"set_meta","title":"T"}"#),
            op(r#"{"op":"add_lesson","module":"m1","key":"l1","title":"L","purpose":"P"}"#),
        ]);
        assert_eq!(report.accepted.len(), 1, "set_meta accepted");
        assert!(
            report.rejected[0]
                .reason
                .contains("unknown module key 'm1'"),
            "{}",
            report.rejected[0].reason
        );
    }

    #[test]
    fn scope_gap_is_a_danger_finding() {
        let draft = OutlineDraft::new(
            kb_brief(),
            samples(),
            Some(ScopeAnalysis {
                scope: "向量代数".into(),
                blocks: vec!["特征值".into()],
                ..Default::default()
            }),
        );
        let mut complete = build_complete_draft(kb_brief());
        complete.scope = draft.scope.clone();
        complete.refresh_audit();
        let kinds = danger_kinds(&complete);
        assert!(kinds.contains(&"scope_gap"), "{kinds:?}");
        assert!(complete.audit_report().contains("特征值"));
    }

    /// 单字 scope 块（真实案例：'栈'、'图'）与任何标题的最长公共子串只有
    /// 1 字符——覆盖条必须按块长截断，否则块永远不可覆盖，audit 死锁。
    #[test]
    fn single_char_scope_block_matches_by_title_containment() {
        let scope = ScopeAnalysis {
            scope: "数据结构".into(),
            blocks: vec!["栈".into()],
            ..Default::default()
        };
        let mut covered = build_complete_draft(kb_brief());
        covered.apply_ops(vec![op(
            r#"{"op":"update_module","key":"m1","title":"栈与队列入门"}"#,
        )]);
        covered.scope = Some(scope.clone());
        covered.refresh_audit();
        assert!(
            !danger_kinds(&covered).contains(&"scope_gap"),
            "{:?}",
            covered.findings
        );

        let mut uncovered = build_complete_draft(kb_brief());
        uncovered.scope = Some(scope);
        uncovered.refresh_audit();
        assert!(
            danger_kinds(&uncovered).contains(&"scope_gap"),
            "{:?}",
            uncovered.findings
        );
        let report = uncovered.audit_report();
        assert!(
            report.contains("containing '栈'"),
            "the finding must state the matching rule: {report}"
        );
    }

    #[test]
    fn description_flow_rejects_lesson_sources() {
        let mut draft = OutlineDraft::new(description_brief(), Vec::new(), None);
        draft.apply_ops(vec![
            op(r#"{"op":"set_meta","title":"T"}"#),
            op(r#"{"op":"add_module","key":"m1","title":"M"}"#),
        ]);
        let report = draft.apply_ops(vec![op(
            r#"{"op":"add_lesson","module":"m1","key":"l1","title":"L","purpose":"P","source":"notes/vectors.md"}"#,
        )]);
        assert!(
            report.rejected[0].reason.contains("no sampled documents"),
            "{}",
            report.rejected[0].reason
        );
    }

    #[test]
    fn kb_flow_rejects_unsampled_source() {
        let mut draft = OutlineDraft::new(kb_brief(), samples(), None);
        draft.apply_ops(vec![
            op(r#"{"op":"set_meta","title":"T"}"#),
            op(r#"{"op":"add_module","key":"m1","title":"M"}"#),
        ]);
        let report = draft.apply_ops(vec![op(
            r#"{"op":"add_lesson","module":"m1","key":"l1","title":"L","purpose":"P","source":"notes/other.md"}"#,
        )]);
        assert!(
            report.rejected[0].reason.contains("is not a sampled file"),
            "{}",
            report.rejected[0].reason
        );
    }

    #[test]
    fn remove_module_cascades_to_lessons() {
        let mut draft = build_complete_draft(kb_brief());
        let report = draft.apply_ops(vec![op(r#"{"op":"remove_module","key":"m1"}"#)]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        assert!(draft.modules.is_empty());
        assert!(draft.lessons.is_empty());
    }

    #[test]
    fn update_lesson_can_replace_purpose_and_clear_source() {
        let mut draft = build_complete_draft(kb_brief());
        let report = draft.apply_ops(vec![op(
            r#"{"op":"update_lesson","key":"l1","purpose":"能手动计算点积","source":null}"#,
        )]);
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        let lesson = draft.lessons.iter().find(|l| l.key == "l1").unwrap();
        assert_eq!(lesson.purpose, "能手动计算点积");
        assert!(lesson.source.is_none());
    }

    #[test]
    fn read_sample_resolves_exact_paths_only() {
        let draft = build_complete_draft(kb_brief());
        assert_eq!(
            draft.read_sample("notes/vectors.md"),
            Some("# 向量\n具有大小和方向的量")
        );
        assert!(draft.read_sample("notes/missing.md").is_none());
    }

    #[test]
    fn duplicate_title_is_a_warning_not_a_danger() {
        let mut draft = OutlineDraft::new(kb_brief(), samples(), None);
        draft.apply_ops(vec![
            op(r#"{"op":"set_meta","title":"T"}"#),
            op(r#"{"op":"add_module","key":"m1","title":"M"}"#),
            op(r#"{"op":"add_lesson","module":"m1","key":"l1","title":"相同","purpose":"P"}"#),
            op(r#"{"op":"add_lesson","module":"m1","key":"l2","title":"相同","purpose":"P"}"#),
        ]);
        let kinds: Vec<&str> = draft
            .findings
            .iter()
            .map(|finding| finding.kind.as_str())
            .collect();
        assert!(kinds.contains(&"duplicate_title"));
        assert!(!kinds.contains(&"duplicate_key"), "{kinds:?}");
    }

    #[test]
    fn patch_report_danger_count_tracks_the_fresh_snapshot() {
        let mut draft = OutlineDraft::new(kb_brief(), samples(), None);
        let report = draft.apply_ops(vec![]);
        assert!(report.danger_count() > 0);
    }
}
