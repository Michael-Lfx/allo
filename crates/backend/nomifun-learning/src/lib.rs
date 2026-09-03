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

pub use learning_graph::{
    GenerateLearningGraphRequest, LearningGraphAgentEngine, LearningGraphAudit, LearningGraphData,
    LearningGraphEdge, LearningGraphNode, LearningGraphRecord, LearningGraphSummary,
};

pub use learning_graph::draft::{
    DraftView, GraphOp, InspectView, NodeListView, NodeQuery, PatchReport, SplitUnit,
    SubgraphDirection, SubgraphView,
};

pub use models::{
    ActivityKind, ActivityView, AttemptResult, ConceptPack, ConceptView, CourseDetail,
    CourseGenerationMode, CourseKind, CoursePack, CourseSummary, DiagnosticItem, DiagnosticPlan,
    DueReview, GenerateCourseRequest, GenerateLessonRequest, LessonStatus, LessonView, ModuleView,
    RateReviewRequest, ReviewRating, ReviewResult, SourceSpan, SubmitAttemptRequest,
    UpdateLessonProgressRequest,
};
pub use routes::learning_routes;
pub use service::LearningService;
pub use state::LearningRouterState;
