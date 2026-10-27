mod completer;
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

pub use switches::{
    is_off_switch_value, is_task_gate_disabled_value, lexicon_enforce_enabled,
    lexicon_enforce_enabled_value, LEXICON_ENFORCE_SWITCH_ENV, TASK_GATE_SWITCH_ENV,
};

pub use completer::LearningCompleter;

pub use events::LearningEventEmitter;

pub use course_outline::{CourseOutlineAgentEngine, KnowledgeBaseBrief, OutlineBrief};

pub use course_outline::draft::{
    OutlineDraftView, OutlineInspectView, OutlineOp, OutlinePatchReport, OutlineQuery,
    OutlineQueryView,
};

pub use lesson_draft::{
    EST_BUDGET_SWITCH_ENV, ExcerptRole, GraphConceptFaces, GraphLessonContext,
    LessonContentAgentEngine, LessonDraftView, LessonExcerpt, LessonGenerationContext,
    LessonInspectView, LessonOp, LessonPatchReport, est_budget_enabled,
    is_est_budget_disabled_value, normalize_capacity_note, render_est_budget_line,
};

pub use generation::{
    Blueprint, BlueprintLesson, BlueprintModule, LessonOutput, SampleDisclosure,
};
pub use generation::{QB_SIMILARITY_SWITCH_ENV, qb_readings};
pub use generation::{
    CONTENT_DISCIPLINE_V2_SWITCH_ENV, content_discipline_v2_enabled,
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
pub use learning_graph::definition_audit;
pub use learning_graph::lexicon;
pub use learning_graph::mounting::fnv1a_64 as learning_graph_hash;
pub use learning_graph::structure_contract::{
    reconcile_doors as reconcile_structure_doors,
    validate_terrain as validate_terrain_artifact,
    validate_structure as validate_structure_artifact,
};
pub use learning_graph::structure_gen::StructureInput;

pub use models::{
    ActivityKind, ActivityView, AttemptResult, CourseDetail,
    CourseGenerationMode, CourseKind, CoursePack, CourseSummary, ConceptAuditCycle,
    ConceptAuditEntry, ConceptAuditReport, ConceptAuditSelfRef, DiagnosticItem, DiagnosticPlan,
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
    LearningGraphGenerationStatus, LearningTaskGateReadout,
};
pub use service::{
    DEFAULT_MAINTENANCE_ENABLED, DEFAULT_MAINTENANCE_INTERVAL_MINUTES,
    FINDINGS_WATERMARK_SWITCH_ENV, MAINTENANCE_INTERVAL_MINUTES,
    is_findings_watermark_disabled_value,
};
pub use models::{
    PRACTICE_BODY_TARGET_CHARS, prose_budget_rules, section_range_rules, visual_distribution,
    visual_menu_text,
};
pub use routes::learning_routes;
pub use service::{CompanionXpSink, GrowthRunStats, LearningService};
pub use learning_graph::mint_evidence::MintResearchStats;
/// 铸名门外部证据开关（m11）：`NOMI_LEARNING_MINT_RESEARCH` 判定与取值。
pub use switches::{MINT_RESEARCH_SWITCH_ENV, mint_research_enabled};
pub use settle::{
    QUIZ_EXCELLENT_RATIO, QUIZ_HALF_RATIO, QUIZ_PASS_RATIO, QUIZ_QUESTION_BUDGET,
    QUIZ_TRIGGER_SETTLEMENTS,
};
pub use state::LearningRouterState;
