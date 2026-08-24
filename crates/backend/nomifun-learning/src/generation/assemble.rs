use super::*;


/// Merge the blueprint and the per-lesson outputs into a `CoursePack`. The
/// requested domain label wins over the blueprint's own when provided.
pub(crate) fn assemble_pack(
    blueprint: Blueprint,
    lesson_outputs: Vec<LessonOutput>,
    request: &GenerateCourseRequest,
) -> CoursePack {
    let mut outputs = lesson_outputs.into_iter();
    let modules = blueprint
        .modules
        .iter()
        .map(|module| ModulePack {
            title: module.title.clone(),
            description: module.description.clone(),
            lessons: module
                .lessons
                .iter()
                .map(|lesson| {
                    let output = outputs.next().expect("lesson outputs match blueprint");
                    LessonPack {
                        title: lesson.title.clone(),
                        summary: output.summary,
                        purpose: lesson.purpose.clone(),
                        estimated_minutes: output.estimated_minutes,
                        source: lesson.source.clone(),
                        concepts: lesson.concepts.clone(),
                        activities: output.activities,
                    }
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
                blueprint.domain
            }
        });
    CoursePack {
        title: blueprint.title,
        description: blueprint.description,
        domain,
        source_kb_id: Some(request.knowledge_base_id.clone()),
        version: blueprint.version.max(1),
        concepts: blueprint.concepts,
        modules,
    }
}


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


pub(crate) fn validate_generated_pack(
    pack: &CoursePack,
    samples: &[(String, String)],
) -> Result<(), String> {
    crate::service::validate_pack(pack).map_err(|error| error.to_string())?;
    let source_paths: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
    for module in &pack.modules {
        for lesson in &module.lessons {
            let source = lesson
                .source
                .as_ref()
                .ok_or_else(|| format!("lesson {} has no source", lesson.title))?;
            if !source_paths.contains(source.path.as_str()) {
                return Err(format!(
                    "lesson {} cites an unsampled source path: {}",
                    lesson.title, source.path
                ));
            }
            if lesson.activities.is_empty() {
                return Err(format!(
                    "lesson {} has no retrieval activity",
                    lesson.title
                ));
            }
        }
    }
    Ok(())
}

