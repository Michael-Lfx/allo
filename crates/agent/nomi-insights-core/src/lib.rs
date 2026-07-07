//! De-identified domain work package contribution pipeline (v3).

pub mod auxiliary {
    pub use nomi_auxiliary::*;
}

pub mod client;
pub mod interest;
pub mod maturity;
pub mod outbox;
pub mod paths;
pub mod redact;
pub mod response;
pub mod sanitize;
pub mod service;
pub mod session_skills;
pub mod skill;
pub mod types;
pub mod work_package;
pub mod work_session;

pub use client::{ContributionClient, FlushResult};
pub use paths::{
    append_audit_event, audit_path, installation_id_path, last_batch_path,
    load_or_create_installation_id, outbox_path, state_dir,
};
pub use redact::{RedactionPattern, Redactor, redact_sensitive_text};
pub use service::ContributionService;
pub use session_skills::{
    SessionSkillSummary, drain_session_skills, record_skill_touch, set_active_session,
};
pub use skill::SkillChangeKind;
pub use types::{
    ContributionBatch, ContributionEnvelope, ContributionType, DomainPoiPayload, DomainWorkPackage,
    INSIGHTS_CONSENT_VERSION, ResolutionPayload, WorkMetricsPayload,
};
pub use work_package::{WorkPackageBuildInput, build_domain_work_package, find_skill_dir_by_slug};
pub use work_session::{spawn_session_end_pipeline, touch_active_session};

/// Fire-and-forget notification after a local skill file changes.
pub fn notify_skill_changed(skill_dir: &std::path::Path, kind: SkillChangeKind) {
    ContributionService::spawn_skill_touch(skill_dir, kind, false);
}
