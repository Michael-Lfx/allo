mod completer;
mod learning_graph;
mod course_outline;
mod events;
mod generation;
mod lesson_draft;
mod models;
mod routes;
mod scheduler;
mod service;
mod state;
mod tutorial;

pub use completer::LearningCompleter;

pub use events::LearningEventEmitter;

pub use course_outline::{CourseOutlineAgentEngine, KnowledgeBaseBrief, OutlineBrief};

pub use course_outline::draft::{
    OutlineDraftView, OutlineInspectView, OutlineOp, OutlinePatchReport, OutlineQuery,
    OutlineQueryView,
};

pub use lesson_draft::{
    ExcerptRole, GraphLessonContext, LessonContentAgentEngine, LessonDraftView, LessonExcerpt,
    LessonGenerationContext, LessonInspectView, LessonOp, LessonPatchReport,
};

pub use generation::{Blueprint, BlueprintLesson, BlueprintModule, LessonOutput, SampleDisclosure};

pub use learning_graph::ConceptTier;
pub use learning_graph::structure_contract::{
    reconcile_doors as reconcile_structure_doors,
    validate_terrain as validate_terrain_artifact,
    validate_structure as validate_structure_artifact,
};
pub use learning_graph::structure_gen::StructureInput;

pub use models::{
    ActivityKind, ActivityView, AttemptResult, CourseDetail,
    CourseGenerationMode, CourseKind, CoursePack, CourseSummary, DiagnosticItem, DiagnosticPlan,
    DueReview, GenerateCourseRequest, GenerateLessonRequest,
    GraphBatchView, GraphBlockedView, GraphConceptRefView, GraphConceptRowView,
    GraphHistoryView, GraphNodeHistoryView, LearningGraphView, LessonStatus,
    LessonView,
    ModuleView, RateReviewRequest, ReviewRating, ReviewResult, SectionKind, SectionPack,
    SectionView, SourceSpan, SubmitAttemptRequest, TeachingStyle, UpdateLessonProgressRequest,
    VISUAL_OPTIONS,
};
pub use models::{
    PRACTICE_BODY_TARGET_CHARS, prose_budget_rules, section_range_rules, visual_distribution,
    visual_menu_text,
};
pub use routes::learning_routes;
pub use service::{GrowthRunStats, LearningService};
pub use state::LearningRouterState;
