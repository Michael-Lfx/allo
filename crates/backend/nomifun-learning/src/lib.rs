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
    GraphLessonContext, LessonContentAgentEngine, LessonDraftView, LessonExcerpt,
    LessonGenerationContext, LessonInspectView, LessonOp, LessonPatchReport,
};

pub use generation::{Blueprint, BlueprintLesson, BlueprintModule, LessonOutput};

pub use learning_graph::ConceptTier;

pub use models::{
    ActivityKind, ActivityView, AttemptResult, CourseDetail,
    CourseGenerationMode, CourseKind, CoursePack, CourseSummary, DiagnosticItem, DiagnosticPlan,
    DueReview, EndpointInput, EndpointUpdateInput, GenerateCourseRequest, GenerateLessonRequest,
    GraphBatchView, GraphConceptRefView, GraphConceptRowView, GraphEndpointView,
    GraphHistoryView, GraphNodeHistoryView, LearningGraphView, LessonStatus, LessonView,
    ModuleView, RateReviewRequest, ReviewRating, ReviewResult, SectionKind, SectionPack,
    SectionView, SourceSpan, SubmitAttemptRequest, TeachingStyle, UpdateLessonProgressRequest,
    VISUAL_OPTIONS,
};
pub use models::{
    PRACTICE_BODY_TARGET_CHARS, prose_budget_rules, section_range_rules, visual_distribution,
    visual_menu_text,
};
pub use routes::learning_routes;
pub use service::LearningService;
pub use state::LearningRouterState;
