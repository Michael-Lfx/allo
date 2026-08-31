use super::*;


/// Outline-only pack for on-demand generation: every lesson keeps its title,
/// purpose, source and concept bindings but no summary or activities. The
/// blueprint and samples are persisted alongside the course so deferred lesson
/// generation can reconstruct the exact grounding context later.
pub(crate) fn assemble_outline_pack(
    blueprint: &Blueprint,
    request: &GenerateCourseRequest,
) -> CoursePack {
    let modules = blueprint
        .modules
        .iter()
        .map(|module| ModulePack {
            title: module.title.clone(),
            description: module.description.clone(),
            lessons: module
                .lessons
                .iter()
                .map(|lesson| LessonPack {
                    title: lesson.title.clone(),
                    summary: String::new(),
                    // No content yet: use the default study-time estimate.
                    estimated_minutes: 10,
                    purpose: lesson.purpose.clone(),
                    source: lesson.source.clone(),
                    concepts: lesson.concepts.clone(),
                    activities: Vec::new(),
                })
                .collect(),
        })
        .collect();
    let domain = request
        .domain
        .as_deref()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if blueprint.domain.trim().is_empty() {
                "general".to_owned()
            } else {
                blueprint.domain.clone()
            }
        });
    CoursePack {
        title: blueprint.title.clone(),
        description: blueprint.description.clone(),
        domain,
        source_kb_id: Some(request.knowledge_base_id.clone()),
        version: blueprint.version.max(1),
        concepts: blueprint.concepts.clone(),
        modules,
    }
}

