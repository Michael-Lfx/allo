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
    completer: &dyn KnowledgeCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    blueprint: &Blueprint,
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
    completer: &dyn KnowledgeCompleter,
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
        let raw = complete(completer, model_override, LESSON_DOCUMENT_SYSTEM, &user)
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


/// Stage 1 prompt: course context, concepts, bridging instruction, the
/// document standard, and the cited excerpt — everything the model needs to
/// write the study document as plain Markdown. No JSON is ever mentioned.
pub(crate) fn build_lesson_document_prompt(
    blueprint: &Blueprint,
    module: &BlueprintModule,
    lesson: &BlueprintLesson,
    module_index: usize,
    lesson_index: usize,
    total_lessons: usize,
    next_lesson_title: Option<&str>,
    excerpt: &str,
) -> String {
    let mut prompt = format!(
        "Course: {}\nModule {}/{}: {}\nLesson {}/{}: {}\nLesson purpose: {}\n\
         Lesson concepts to cover:\n",
        blueprint.title,
        module_index + 1,
        blueprint.modules.len(),
        module.title,
        lesson_index + 1,
        total_lessons,
        lesson.title,
        lesson.purpose.trim()
    );
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
/// order, each introduced by a `## ` heading line.
pub(super) fn validate_lesson_document(summary: &str) -> Result<(), String> {
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

