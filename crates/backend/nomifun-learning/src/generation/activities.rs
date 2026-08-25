use super::completer::complete;
use super::parser::{normalize_prompt, parse_json_object};
use super::*;


/// Stage 2 of one lesson: produce `estimated_minutes` + activities as a
/// small JSON object, grounded in the finished document.
pub(super) async fn generate_lesson_activities(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    prompt: &str,
    blueprint: &Blueprint,
    lesson: &BlueprintLesson,
) -> Result<ActivitiesOutput, String> {
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            prompt.to_owned()
        } else {
            format!(
                "{prompt}\n\nThe previous activities JSON was rejected: {last_error}\n\
                 Return a corrected JSON now, keeping every activity field complete."
            )
        };
        let raw = complete(
            completer,
            model_override,
            LESSON_SYSTEM,
            &user,
            ACTIVITIES_MAX_TOKENS,
        )
        .await
        .map_err(|error| error.to_string())?;
        match parse_json_object::<ActivitiesOutput>(&raw) {
            Ok(output) => match validate_lesson_activities(&output.activities, blueprint, lesson) {
                Ok(()) => return Ok(output),
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}


/// One question already present in the lesson, listed in the generation
/// prompt so the model produces something novel instead of a paraphrase.
#[derive(Debug, Clone)]
pub(crate) struct ExistingLessonQuestion {
    pub kind: ActivityKind,
    pub prompt: String,
    pub answer: serde_json::Value,
    pub explanation: String,
}


/// Generates ONE additional activity for an already-generated lesson. The
/// lesson body is fixed, so unlike [`generate_lesson`] there is no document
/// stage: a single call produces one activity of the learner-chosen kind,
/// grounded in the finished document plus the cited excerpt, and validated
/// for shape and novelty against the questions the lesson already has.
/// The result is a draft for preview — nothing is persisted here.
pub(crate) async fn generate_lesson_activity(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    kind: ActivityKind,
    focus: &str,
    course_title: &str,
    module_title: &str,
    lesson_title: &str,
    concepts: &[ConceptPack],
    lesson_concept_keys: &[String],
    summary: &str,
    excerpt: &str,
    existing_questions: &[ExistingLessonQuestion],
) -> Result<ActivityPack, String> {
    let prompt = build_lesson_activity_prompt(
        kind,
        focus,
        course_title,
        module_title,
        lesson_title,
        concepts,
        lesson_concept_keys,
        summary,
        excerpt,
        existing_questions,
    );
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            prompt.clone()
        } else {
            format!(
                "{prompt}\n\nThe previous activity JSON was rejected: {last_error}\n\
                 Return a corrected JSON now, keeping every field complete."
            )
        };
        let raw = complete(
            completer,
            model_override,
            LESSON_ACTIVITY_SYSTEM,
            &user,
            SINGLE_ACTIVITY_MAX_TOKENS,
        )
        .await
        .map_err(|error| error.to_string())?;
        match parse_json_object::<ActivityPack>(&raw) {
            Ok(activity) => {
                match validate_generated_activity(&activity, kind, lesson_concept_keys, existing_questions) {
                    Ok(()) => return Ok(activity),
                    Err(error) => last_error = error,
                }
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}


/// Prompt for one additional activity: the finished lesson document in full,
/// the cited excerpt, the lesson's concepts, the learner's optional focus
/// hint, and every existing question — with a hard novelty requirement.
fn build_lesson_activity_prompt(
    kind: ActivityKind,
    focus: &str,
    course_title: &str,
    module_title: &str,
    lesson_title: &str,
    concepts: &[ConceptPack],
    lesson_concept_keys: &[String],
    summary: &str,
    excerpt: &str,
    existing_questions: &[ExistingLessonQuestion],
) -> String {
    let mut prompt = format!(
        "Course: {}\nModule: {}\nLesson: {}\nLesson concepts (bind only these keys; \
         leave \"concepts\" empty to bind the whole lesson):\n",
        course_title, module_title, lesson_title
    );
    for concept in concepts {
        if lesson_concept_keys.iter().any(|key| key == &concept.key) {
            prompt.push_str(&format!(
                "- {} ({}) — {}\n",
                concept.key,
                concept.title,
                concept.description.trim()
            ));
        }
    }
    prompt.push_str("Finished lesson document (design the question to verify exactly what it teaches):\n");
    prompt.push_str("--- DOCUMENT START ---\n");
    prompt.push_str(summary);
    prompt.push_str("\n--- DOCUMENT END ---\n\n");
    prompt.push_str(&format!(
        "Cited file excerpt (the question must stay grounded in it):\n--- FILE: ---\n{excerpt}\n\n"
    ));
    if !focus.trim().is_empty() {
        prompt.push_str(&format!(
            "Focus hint from the learner: {}\n",
            focus.trim()
        ));
    }
    if existing_questions.is_empty() {
        prompt.push_str("Existing questions in this lesson: none yet — you write the first.\n");
    } else {
        prompt.push_str(&format!(
            "Existing questions in this lesson ({}):\n",
            existing_questions.len()
        ));
        for (index, question) in existing_questions.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. [{}] {}\n   answer: {}\n   explanation: {}\n",
                index + 1,
                question.kind.as_str(),
                question.prompt.trim(),
                question.answer,
                question.explanation.trim()
            ));
        }
        prompt.push_str(
            "The new question must NOT repeat or closely resemble any of these: \
             cover a knowledge point none of them tests, or approach the same point \
             from a different angle.\n",
        );
    }
    prompt.push_str(&format!(
        "Design ONE new {} activity as JSON now.",
        kind.as_str()
    ));
    prompt
}


/// Single-activity validation: the requested kind's exact shape plus concept
/// binding and a novelty check against the lesson's existing questions.
fn validate_generated_activity(
    activity: &ActivityPack,
    kind: ActivityKind,
    lesson_concept_keys: &[String],
    existing_questions: &[ExistingLessonQuestion],
) -> Result<(), String> {
    if activity.kind != kind {
        return Err(format!(
            "expected a {} activity, got {}",
            kind.as_str(),
            activity.kind.as_str()
        ));
    }
    if activity.prompt.trim().is_empty() {
        return Err("activity prompt is empty".into());
    }
    for concept in &activity.concepts {
        if !lesson_concept_keys.iter().any(|key| key == concept) {
            return Err(format!(
                "activity \"{}\" references concept {concept} not bound to this lesson",
                activity.prompt
            ));
        }
    }
    match kind {
        ActivityKind::SingleChoice => {
            if !(2..=4).contains(&activity.options.len()) {
                return Err(format!(
                    "single_choice \"{}\" has {} options, expected 2-4",
                    activity.prompt,
                    activity.options.len()
                ));
            }
            let mut seen = HashSet::new();
            for option in &activity.options {
                if option.trim().is_empty() || !seen.insert(option.trim().to_lowercase()) {
                    return Err(format!(
                        "single_choice \"{}\" options must be distinct and non-empty",
                        activity.prompt
                    ));
                }
            }
            let Some(answer) = activity.answer.as_str() else {
                return Err(format!(
                    "single_choice \"{}\" answer must be a string",
                    activity.prompt
                ));
            };
            if !activity.options.iter().any(|option| option == answer) {
                return Err(format!(
                    "single_choice \"{}\" answer does not match any option",
                    activity.prompt
                ));
            }
        }
        ActivityKind::TrueFalse => {
            if !activity.answer.is_boolean() {
                return Err(format!(
                    "true_false \"{}\" answer must be a boolean",
                    activity.prompt
                ));
            }
        }
        ActivityKind::Reflection => {
            if !activity.answer.is_null() {
                return Err(format!(
                    "reflection \"{}\" answer must be null",
                    activity.prompt
                ));
            }
        }
        ActivityKind::FillInBlank => {
            if !activity.prompt.contains("___") {
                return Err(format!(
                    "fill_in_blank \"{}\" prompt must contain a ___ blank",
                    activity.prompt
                ));
            }
            let Some(answers) = activity.answer.as_array() else {
                return Err(format!(
                    "fill_in_blank \"{}\" answer must be a JSON array of accepted answers",
                    activity.prompt
                ));
            };
            if answers.is_empty() || answers.len() > 3 {
                return Err(format!(
                    "fill_in_blank \"{}\" must have 1-3 accepted answers",
                    activity.prompt
                ));
            }
            if answers.iter().any(|accepted| {
                !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
            }) {
                return Err(format!(
                    "fill_in_blank \"{}\" accepted answers must be non-empty strings",
                    activity.prompt
                ));
            }
            if activity
                .distractors
                .iter()
                .all(|distractor| distractor.trim().is_empty())
            {
                return Err(format!(
                    "fill_in_blank \"{}\" must provide at least one near-synonym distractor",
                    activity.prompt
                ));
            }
        }
    }
    let normalized = normalize_prompt(&activity.prompt);
    for existing in existing_questions {
        let existing_normalized = normalize_prompt(&existing.prompt);
        let trivial = normalized.len().min(existing_normalized.len()) <= 16;
        if normalized == existing_normalized
            || (!trivial
                && (normalized.contains(&existing_normalized)
                    || existing_normalized.contains(&normalized)))
        {
            return Err(format!(
                "the question duplicates or closely resembles an existing question: \"{}\"",
                existing.prompt
            ));
        }
    }
    Ok(())
}


/// Stage 2 prompt: the finished lesson document is passed in full so the
/// activities verify exactly what it teaches, never a parallel invention.
pub(crate) fn build_activities_prompt(
    blueprint: &Blueprint,
    lesson: &BlueprintLesson,
    summary: &str,
    excerpt: &str,
) -> String {
    let mut prompt = format!(
        "Course: {}\nLesson: {}\nLesson concepts (use these exact keys when binding activities; \
         the reflection question(s) must cover ALL of them):\n",
        blueprint.title,
        lesson.title,
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
    prompt.push_str("Finished lesson document (design activities that verify exactly what it teaches):\n");
    prompt.push_str("--- DOCUMENT START ---\n");
    prompt.push_str(summary);
    prompt.push_str("\n--- DOCUMENT END ---\n\n");
    prompt.push_str(&format!(
        "Cited file excerpt (questions must stay grounded in it):\n\
         --- FILE: {} ---\n{excerpt}\n\nDesign the activity JSON now.",
        lesson
            .source
            .as_ref()
            .map(|source| source.path.as_str())
            .unwrap_or_default()
    ));
    prompt
}


/// Activity-stage validation: count floors, concept binding, and per-kind
/// shape rules — everything that made the historical single-call validator
/// reject weak activity output.
pub(super) fn validate_lesson_activities(
    activities: &[ActivityPack],
    blueprint: &Blueprint,
    lesson: &BlueprintLesson,
) -> Result<(), String> {
    if activities.len() < LESSON_MIN_ACTIVITIES {
        return Err(format!(
            "lesson has {} activities, expected at least {LESSON_MIN_ACTIVITIES}",
            activities.len()
        ));
    }
    let objective = activities
        .iter()
        .filter(|activity| activity.kind != ActivityKind::Reflection)
        .count();
    if objective < LESSON_MIN_OBJECTIVE_ACTIVITIES {
        return Err(format!(
            "lesson has {objective} objective activities, expected at least {LESSON_MIN_OBJECTIVE_ACTIVITIES}"
        ));
    }
    let reflections = activities.len() - objective;
    if reflections > LESSON_MAX_REFLECTION_ACTIVITIES {
        return Err(format!(
            "lesson has {reflections} reflection questions, expected at most {LESSON_MAX_REFLECTION_ACTIVITIES}"
        ));
    }
    let concept_keys: HashSet<&str> = blueprint
        .concepts
        .iter()
        .map(|concept| concept.key.as_str())
        .collect();
    let lesson_concepts: HashSet<&str> = lesson.concepts.iter().map(String::as_str).collect();
    for activity in activities {
        if activity.prompt.trim().is_empty() {
            return Err("activity prompt is empty".into());
        }
        for concept in &activity.concepts {
            if !concept_keys.contains(concept.as_str()) {
                return Err(format!(
                    "activity \"{}\" references unknown concept {concept}",
                    activity.prompt
                ));
            }
            if !lesson_concepts.contains(concept.as_str()) {
                return Err(format!(
                    "activity \"{}\" references concept {concept} not bound to this lesson",
                    activity.prompt
                ));
            }
        }
        match activity.kind {
            ActivityKind::SingleChoice => {
                if !(3..=5).contains(&activity.options.len()) {
                    return Err(format!(
                        "single_choice \"{}\" has {} options, expected 3-5",
                        activity.prompt,
                        activity.options.len()
                    ));
                }
                let Some(answer) = activity.answer.as_str() else {
                    return Err(format!(
                        "single_choice \"{}\" answer must be a string",
                        activity.prompt
                    ));
                };
                if !activity.options.iter().any(|option| option == answer) {
                    return Err(format!(
                        "single_choice \"{}\" answer does not match any option",
                        activity.prompt
                    ));
                }
            }
            ActivityKind::TrueFalse => {
                if !activity.answer.is_boolean() {
                    return Err(format!(
                        "true_false \"{}\" answer must be a boolean",
                        activity.prompt
                    ));
                }
            }
            ActivityKind::Reflection => {
                if !activity.answer.is_null() {
                    return Err(format!(
                        "reflection \"{}\" answer must be null",
                        activity.prompt
                    ));
                }
            }
            ActivityKind::FillInBlank => {
                if !activity.prompt.contains("___") {
                    return Err(format!(
                        "fill_in_blank \"{}\" prompt must contain a ___ blank",
                        activity.prompt
                    ));
                }
                let Some(answers) = activity.answer.as_array() else {
                    return Err(format!(
                        "fill_in_blank \"{}\" answer must be a JSON array of accepted answers",
                        activity.prompt
                    ));
                };
                if answers.is_empty() || answers.len() > 3 {
                    return Err(format!(
                        "fill_in_blank \"{}\" must have 1-3 accepted answers",
                        activity.prompt
                    ));
                }
                if answers.iter().any(|accepted| {
                    !accepted.as_str().is_some_and(|text| !text.trim().is_empty())
                }) {
                    return Err(format!(
                        "fill_in_blank \"{}\" accepted answers must be non-empty strings",
                        activity.prompt
                    ));
                }
                if activity
                    .distractors
                    .iter()
                    .all(|distractor| distractor.trim().is_empty())
                {
                    return Err(format!(
                        "fill_in_blank \"{}\" must provide at least one near-synonym distractor",
                        activity.prompt
                    ));
                }
            }
        }
    }
    Ok(())
}

