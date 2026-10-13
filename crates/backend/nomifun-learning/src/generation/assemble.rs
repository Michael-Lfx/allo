use super::*;


/// Outline-only pack for on-demand generation: every lesson keeps its title,
/// purpose and source but no summary or activities. The blueprint and
/// samples are persisted alongside the course so deferred lesson generation
/// can reconstruct the exact grounding context later.
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
                    // 引用清单（t4 评审补齐）：列表原样携带（首个 = 兼容位），
                    // 落库时列表进 `source_paths_json`、首个进三列。
                    source: lesson.source.clone(),
                    sources: lesson.sources.clone(),
                    activities: Vec::new(),
                    sections: Vec::new(),
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
    // 课程名默认即目标文本（t1 裁决：不引入 LLM 派名）；无目标时沿用模型
    // 自定的蓝图标题（旧路径零变化）。
    let title = request
        .goal_text()
        .map(|goal| crate::models::goal_course_title(&goal))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| blueprint.title.clone());
    CoursePack {
        title,
        description: blueprint.description.clone(),
        domain,
        // Absent for the description flow: no knowledge base backs the course.
        source_kb_id: request.knowledge_base_id.clone(),
        version: blueprint.version.max(1),
        modules,
        teaching_style: request.teaching_style.unwrap_or_default(),
    }
}

