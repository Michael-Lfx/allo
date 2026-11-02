mod completer;
mod envelope;
mod learning_graph;
mod course_outline;
mod events;
mod generation;
mod lesson_draft;
mod models;
mod routes;
mod scheduler;
mod settle;
mod service;
mod state;
mod switches;
mod tutorial;
mod verify_math;

pub use switches::{
    is_auto_refill_disabled_value, is_off_switch_value, is_task_gate_disabled_value,
    lexicon_enforce_enabled, lexicon_enforce_enabled_value, open_judge_enabled,
    open_judge_enabled_value, AUTO_REFILL_SWITCH_ENV, LEXICON_ENFORCE_SWITCH_ENV,
    OPEN_JUDGE_SWITCH_ENV, TASK_GATE_SWITCH_ENV,
};

pub use completer::{LearningCompleter, LlmUsageDelta, LlmUsageMeter};

pub use events::LearningEventEmitter;
pub use events::{EVENT_COURSE_GENERATION, EVENT_LESSON_GENERATION, phase};

pub use course_outline::{CourseOutlineAgentEngine, KnowledgeBaseBrief, OutlineBrief};

pub use course_outline::draft::{
    OutlineDraftView, OutlineInspectView, OutlineOp, OutlinePatchReport, OutlineQuery,
    OutlineQueryView,
};

pub use lesson_draft::{
    EST_BUDGET_SWITCH_ENV, ExcerptRole, GraphConceptFaces, GraphLessonContext, LessonContentAgentEngine,
    LessonDraftView, LessonExcerpt, LessonGenerationContext, LessonInspectView, LessonOp,
    LessonPatchReport, LessonPrecedent, LessonSectionBodySummary, est_budget_enabled,
    is_est_budget_disabled_value, normalize_capacity_note, render_est_budget_line,
    render_known_names_line, render_precedent_block, render_teaches_tier_line,
};

pub use generation::{
    Blueprint, BlueprintLesson, BlueprintModule, LessonOutput, SampleDisclosure,
};
pub use generation::{QB_SIMILARITY_SWITCH_ENV, qb_readings};
// R14 EN 块随开关常量再导出（w1 评审修复）：ai-agent 的跨宿互钉测试经
// crate 根引用（generation 模块私有，类型/常量公开先例同 SampleDisclosure）。
pub use generation::{
    CONTENT_DISCIPLINE_V2_BLOCK, CONTENT_DISCIPLINE_V2_SWITCH_ENV, content_discipline_v2_enabled,
    is_content_discipline_v2_disabled_value,
};
pub use generation::{
    KbFileKind, KbInventory, KbInventoryBase, KbInventoryFile, KbScanScope, KbTriageCounts,
    KbTriageDuplicate, KbTriageExclusion, KbTriageFile, KbTriageReport, KbTriageRole,
    run_kb_triage,
};
pub use generation::{
    ChapterContentType, ConceptEdgeRelation, ConceptGraph, ConceptGraphEdge, ConceptGraphNode,
    CoverageExclusion, CoverageFile, CoverageLedger, CoverageTopic, FileCoverageStatus,
    PlannedChapter, SourceAnchor, SourceCount, SourceEvidence, SourcePlan, TopicStatus,
    run_kb_structure,
};
pub use generation::{
    ExpansionChapterReport, ExpansionFileReport, ExpansionLessonReport, ExpansionOutcome,
    ExpansionTopicReport, SourceCoverage, run_kb_expansion,
};
pub use generation::{
    COURSE_SOURCE_PLAN_SNAPSHOT_VERSION, CourseSourcePlanSnapshot, SNAPSHOT_HEADINGS_MAX,
    SNAPSHOT_PREVIEW_MAX_CHARS,
};

pub use learning_graph::ConceptTier;
pub use learning_graph::basis_declaration::{
    BASIS_DECLARATION_SWITCH_ENV, DECLARATION_SOURCE, DECLARATION_SYSTEM, DeclaredConceptView,
    is_basis_declaration_disabled_value,
};
pub use learning_graph::AutoRefillVerdict;
pub use learning_graph::definition_audit;
pub use learning_graph::lexicon;
pub use learning_graph::mounting::{
    fnv1a_64 as learning_graph_hash, hash_structure_column, mounts_stale,
    STRUCTURE_HASH_VERSION, STRUCTURE_HASH_VERSION_V1,
};
pub use learning_graph::structure_contract::{
    reconcile_doors as reconcile_structure_doors,
    validate_terrain as validate_terrain_artifact,
    validate_structure as validate_structure_artifact,
};
pub use learning_graph::structure_gen::StructureInput;
/// 门位标签读数单源（e2）：快照 info/warn 行与 smoke structure 档 OK 行共用。
pub use learning_graph::structure_gen::count_door_labeled_blocks;

pub use models::{
    ActivityKind, ActivityView, AttemptResult, BasisConceptView, BasisView, ConceptAuditCycle,
    ConceptAuditEntry, ConceptAuditReport, ConceptAuditSelfRef, CourseDetail,
    CourseGenerationMode, CourseKind, CoursePack, CourseSummary, DeclareBasisRequest,
    DeclarationResultView, DiagnosticItem, DiagnosticPlan,
    DueReview, GenerateCourseRequest, GenerateLessonRequest,
    GraphBatchView, GraphBlockedView, GraphConceptRefView, GraphConceptRowView,
    GraphHistoryView, GraphNodeHistoryView, LearningGraphView, LessonStatus,
    LessonView,
    ModuleView, QuizGradeFeedback, QuizProgressView, QuizRoundNodeReport, QuizRoundQuestionView,
    QuizRoundReport, QuizRoundView, RateReviewRequest, ReviewRating, ReviewResult, SectionKind,
    SectionPack,
    SectionView, SourceSpan, SubmitAttemptRequest, TeachingStyle, UpdateLessonProgressRequest,
    VISUAL_OPTIONS, XpCalendar, XpCalendarDay, XpReading,
};
pub use models::{
    MaintenanceCourseOutcome, MaintenanceNeedsView, MaintenanceSettings, MaintenanceTickReport,
    LearningGraphGenerationStatus, LearningTaskGateReadout, GrowthVerdictView,
};
pub use service::{
    DEFAULT_AUTO_REFILL_ENABLED, DEFAULT_MAINTENANCE_ENABLED, DEFAULT_MAINTENANCE_INTERVAL_MINUTES,
    FINDINGS_WATERMARK_SWITCH_ENV, MAINTENANCE_INTERVAL_MINUTES,
    is_findings_watermark_disabled_value,
};
pub use models::{
    PRACTICE_BODY_TARGET_CHARS, prose_budget_rules, section_range_rules, visual_distribution,
    visual_menu_text,
};
/// verify_math 断言核验接口（票 w1/蓝图 4.1）：求值器与 claim 核验单源在
/// learning（A1），ai-agent 工具 handler 直调——零新依赖。
pub use verify_math::{ClaimVerdict, ExprError, MathClaim, eval_expr, verify_claim};
pub use routes::learning_routes;
pub use service::{CompanionXpSink, GrowthRunStats, LearningService};
/// 课时生成双模面（e4）：异步臂 202 响应体 + 门面重试环读数（smoke 判据）。
pub use service::{LessonGenerationAccepted, LessonRunStats};
/// 判分观测面脱敏占位（n4 D9）：smoke grade 档注册进 `ArchivingCompleter`
/// 时引用同一常量——占位文本单源（判底/作答原文不进观测面）。
pub use service::{REDACTED_GROUNDING_PLACEHOLDER, REDACTED_RESPONSE_PLACEHOLDER};
pub use learning_graph::mint_evidence::MintResearchStats;
/// 铸名门外部证据开关（m11）：`NOMI_LEARNING_MINT_RESEARCH` 判定与取值。
pub use switches::{MINT_RESEARCH_SWITCH_ENV, mint_research_enabled};
pub use settle::{
    QUIZ_EXCELLENT_RATIO, QUIZ_HALF_RATIO, QUIZ_PASS_RATIO, QUIZ_QUESTION_BUDGET,
    QUIZ_TRIGGER_SETTLEMENTS,
};
pub use state::LearningRouterState;
