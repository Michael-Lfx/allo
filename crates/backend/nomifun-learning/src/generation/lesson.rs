use super::activities::{build_activities_prompt, generate_lesson_activities};
use super::completer::complete;
use super::parser::strip_markdown_fences;
use super::*;
#[cfg(test)]
use super::activities::validate_lesson_activities;


/// One lesson in two stages, each with at most one targeted retry: first the
/// study document is generated as plain Markdown (no JSON wrapper, so the
/// long-form text can never be lost to escaping or truncation errors), then a
/// separate small call produces the activities JSON from the finished
/// document. The historical single-call JSON — document plus activities in
/// one object — was the dominant source of parse failures; splitting it keeps
/// the failure-prone JSON payload tiny while each stage keeps its own retry.
pub(crate) async fn generate_lesson(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    blueprint: &Blueprint,
    samples: &[(String, String)],
    module: &BlueprintModule,
    lesson: &BlueprintLesson,
    module_index: usize,
    lesson_index: usize,
    total_lessons: usize,
    next_lesson_title: Option<&str>,
    excerpt: &str,
) -> Result<LessonOutput, String> {
    // Stage 1: the study document as plain Markdown.
    let document_prompt = build_lesson_document_prompt(
        blueprint,
        samples,
        module,
        lesson,
        module_index,
        lesson_index,
        total_lessons,
        next_lesson_title,
        excerpt,
    );
    let summary = generate_lesson_document(completer, model_override, &document_prompt).await?;

    // Stage 2: activities and study time as a small JSON object grounded in
    // the finished document.
    let activities_prompt = build_activities_prompt(blueprint, lesson, &summary, excerpt);
    let activities =
        generate_lesson_activities(completer, model_override, &activities_prompt, blueprint, lesson)
            .await?;

    Ok(LessonOutput {
        summary,
        estimated_minutes: activities.estimated_minutes,
        activities: activities.activities,
    })
}


/// Stage 1 of one lesson: produce the study document as plain Markdown. A
/// failed attempt is retried once with the concrete validation error so the
/// model fixes structure instead of shrinking output.
async fn generate_lesson_document(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    prompt: &str,
) -> Result<String, String> {
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            prompt.to_owned()
        } else {
            format!(
                "{prompt}\n\nThe previous lesson document was rejected: {last_error}\n\
                 Return a corrected document now: start directly with its first `## ` heading \
                 and keep the long-form length."
            )
        };
        let raw = complete(
            completer,
            model_override,
            LESSON_DOCUMENT_SYSTEM,
            &user,
            LESSON_DOCUMENT_MAX_TOKENS,
        )
        .await
        .map_err(|error| error.to_string())?;
        let document = strip_markdown_fences(&raw);
        match validate_lesson_document(&document) {
            Ok(()) => return Ok(document),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}


/// Per-adjacent-lesson excerpt budget: neighbors are reference material for
/// de-duplication and bridging, never the grounding — this hard cap keeps the
/// added context bounded even when the sampled files are huge (the "if the
/// context is already rich enough, add nothing" contract).
pub(crate) const ADJACENT_EXCERPT_MAX_CHARS: usize = 1000;

/// Trim to at most `max` characters (appending an ellipsis when truncated).
fn truncate_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(max).collect();
    format!("{truncated}……")
}

/// The FULL course outline as a compact tree with the current lesson marked.
/// The anti-duplication / no-scope-creep contract: the model sees every
/// sibling lesson before writing a word.
pub(crate) fn build_outline_tree(
    blueprint: &Blueprint,
    module_position: usize,
    lesson_position: usize,
) -> String {
    let mut global = 0usize;
    let mut lines: Vec<String> = Vec::with_capacity(blueprint.modules.len() + 8);
    for (module_index, module) in blueprint.modules.iter().enumerate() {
        lines.push(format!(
            "模块 {}/{}：{}",
            module_index + 1,
            blueprint.modules.len(),
            module.title.trim()
        ));
        for (lesson_index, lesson) in module.lessons.iter().enumerate() {
            global += 1;
            let current = module_index == module_position && lesson_index == lesson_position;
            lines.push(format!(
                "  {}. {} — {}{}",
                global,
                lesson.title.trim(),
                lesson.purpose.trim(),
                if current { "（本课时）" } else { "" }
            ));
        }
    }
    lines.join("\n")
}

/// One adjacent lesson (prev/next in the GLOBAL lesson sequence): title and
/// purpose always, plus the cited excerpt truncated to the budget when the
/// lesson has a sampled source.
struct AdjacentLesson {
    label: &'static str,
    title: String,
    purpose: String,
    excerpt: Option<String>,
}

/// The global prev/next neighbors of the current lesson.
fn adjacent_lessons(
    blueprint: &Blueprint,
    samples: &[(String, String)],
    module_position: usize,
    lesson_position: usize,
) -> Vec<AdjacentLesson> {
    let flat: Vec<(usize, usize)> = blueprint
        .modules
        .iter()
        .enumerate()
        .flat_map(|(module_index, module)| {
            (0..module.lessons.len()).map(move |lesson_index| (module_index, lesson_index))
        })
        .collect();
    let Some(current) = flat
        .iter()
        .position(|(module_index, lesson_index)| {
            *module_index == module_position && *lesson_index == lesson_position
        })
    else {
        return Vec::new();
    };
    let neighbors = [
        ("上一课时", current.checked_sub(1)),
        ("下一课时", Some(current + 1).filter(|next| *next < flat.len())),
    ];
    neighbors
        .into_iter()
        .filter_map(|(label, neighbor_index)| {
            let &(module_index, lesson_index) = flat.get(neighbor_index?)?;
            let lesson = &blueprint.modules[module_index].lessons[lesson_index];
            let excerpt = lesson.source.as_ref().and_then(|source| {
                samples
                    .iter()
                    .find(|(path, _)| path == &source.path)
                    .map(|(_, text)| truncate_chars(text, ADJACENT_EXCERPT_MAX_CHARS))
            });
            Some(AdjacentLesson {
                label,
                title: lesson.title.trim().to_owned(),
                purpose: lesson.purpose.trim().to_owned(),
                excerpt,
            })
        })
        .collect()
}

/// Render the adjacent-lesson reference section (empty when there is nothing
/// to reference). Shared by the fallback prompt builder and the engine path —
/// the service pre-renders it into `LessonGenerationContext`.
pub(crate) fn build_adjacent_context(
    blueprint: &Blueprint,
    samples: &[(String, String)],
    module_position: usize,
    lesson_position: usize,
) -> String {
    let lessons = adjacent_lessons(blueprint, samples, module_position, lesson_position);
    if lessons.is_empty() {
        return String::new();
    }
    let mut lines =
        vec!["相邻课时参考（只做衔接与避重：不要重复其内容，也不要越界代讲）：".to_owned()];
    for lesson in lessons {
        lines.push(format!(
            "- {}「{}」— {}",
            lesson.label, lesson.title, lesson.purpose
        ));
        if let Some(excerpt) = &lesson.excerpt {
            lines.push(format!("  原文摘录（节选）：{excerpt}"));
        }
    }
    lines.join("\n")
}

/// Stage 1 prompt: course context, concepts, bridging instruction, the
/// document standard, and the cited excerpt — everything the model needs to
/// write the study document as plain Markdown. No JSON is ever mentioned.
pub(crate) fn build_lesson_document_prompt(
    blueprint: &Blueprint,
    samples: &[(String, String)],
    module: &BlueprintModule,
    lesson: &BlueprintLesson,
    module_index: usize,
    lesson_index: usize,
    total_lessons: usize,
    next_lesson_title: Option<&str>,
    excerpt: &str,
) -> String {
    let mut prompt = format!(
        "Course: {}\nModule {}/{}: {}\nLesson {}/{}: {}\nLesson purpose: {}\n",
        blueprint.title,
        module_index + 1,
        blueprint.modules.len(),
        module.title,
        lesson_index + 1,
        total_lessons,
        lesson.title,
        lesson.purpose.trim()
    );
    prompt.push_str(&format!(
        "Full course outline (「本课时」 marks the current lesson — stay within \
         its scope, do not teach later lessons here, and do not repeat adjacent \
         lessons):\n{}\n",
        build_outline_tree(blueprint, module_index, lesson_index)
    ));
    prompt.push_str("Lesson concepts to cover:\n");
    for concept_key in &lesson.concepts {
        let concept = blueprint
            .concepts
            .iter()
            .find(|concept| &concept.key == concept_key);
        if let Some(concept) = concept {
            prompt.push_str(&format!(
                "- {} ({}) — {}\n",
                concept.key,
                concept.title,
                concept.description.trim()
            ));
        } else {
            prompt.push_str(&format!("- {concept_key}\n"));
        }
    }
    if let Some(next) = next_lesson_title {
        prompt.push_str(&format!(
            "Next lesson in this module: \"{next}\" — bridge to it in the closing sentence.\n"
        ));
    } else {
        prompt.push_str("This is the last lesson of the module; close with a wrap-up of the module's ideas.\n");
    }
    let adjacent = build_adjacent_context(blueprint, samples, module_index, lesson_index);
    if !adjacent.is_empty() {
        prompt.push_str(&format!("{adjacent}\n\n"));
    }
    prompt.push_str(&format!(
        "{LESSON_DOCUMENT_STANDARD}\n\nCited file excerpt (the lesson must stay grounded in it):\n\
         --- FILE: {} ---\n{excerpt}\n\nWrite the lesson document now.",
        lesson
            .source
            .as_ref()
            .map(|source| source.path.as_str())
            .unwrap_or_default()
    ));
    prompt
}


/// Combined validation over a complete lesson output — the document rules
/// plus the activity rules. Test-only: production goes through the split
/// per-stage validators.
#[cfg(test)]
pub(super) fn validate_lesson(
    output: &LessonOutput,
    blueprint: &Blueprint,
    _module: &BlueprintModule,
    lesson: &BlueprintLesson,
) -> Result<(), String> {
    validate_lesson_document(&output.summary)?;
    validate_lesson_activities(&output.activities, blueprint, lesson)
}


/// Document-stage validation: a hard length floor plus the three required
/// sections — 描述/例子/验证 (English variants accepted) — appearing in
/// order, each introduced by a `## ` heading line. `pub(crate)`: the lesson
/// draft audit reuses this validator verbatim so both pipelines enforce the
/// identical document contract.
pub(crate) fn validate_lesson_document(summary: &str) -> Result<(), String> {
    let char_count = summary.chars().filter(|c| !c.is_whitespace()).count();
    if char_count < LESSON_SUMMARY_MIN_CHARS {
        return Err(format!(
            "summary is {char_count} non-whitespace characters, expected at least {LESSON_SUMMARY_MIN_CHARS}"
        ));
    }
    const REQUIRED_SECTIONS: [(&str, &[&str]); 3] = [
        ("描述", &["描述", "Description"]),
        ("例子", &["例子", "Examples"]),
        ("验证", &["验证", "Verification"]),
    ];
    let lines: Vec<&str> = summary.lines().collect();
    let mut seen = 0usize;
    for (label, names) in REQUIRED_SECTIONS {
        let at = lines[seen..].iter().position(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("## ")
                && names
                    .iter()
                    .any(|name| trimmed[3..].trim_start().starts_with(name))
        });
        match at {
            Some(offset) => seen += offset + 1,
            None => {
                return Err(format!(
                    "document is missing the required \"## {label}\" section; \
                     the three required sections must appear in order, each on its own `## ` heading line"
                ));
            }
        }
    }
    Ok(())
}

