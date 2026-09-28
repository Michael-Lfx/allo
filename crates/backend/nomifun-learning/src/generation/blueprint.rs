use super::completer::complete;
use super::parser::parse_json_object;
use super::*;


/// One blueprint call with at most one targeted retry: the concrete validation
/// error is fed back so the model fixes structure instead of shrinking output.
pub(crate) async fn generate_blueprint(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    prompt: &str,
    samples: &[(String, String)],
) -> Result<Blueprint, AppError> {
    let mut system = BLUEPRINT_SYSTEM.to_owned();
    if samples.is_empty() {
        // Description flow: no sampled documents exist, so the file-citation
        // rule is replaced by brief grounding and lessons omit `source`.
        system.push('\n');
        system.push_str(BLUEPRINT_NO_SAMPLE_RULE);
    }
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            prompt.to_owned()
        } else {
            format!(
                "{prompt}\n\nThe previous blueprint was rejected: {last_error}\n\
                 Return a corrected blueprint JSON now."
            )
        };
        let raw = complete(
            completer,
            model_override,
            &system,
            &user,
            BLUEPRINT_MAX_TOKENS,
        )
        .await?;
        match parse_json_object::<Blueprint>(&raw) {
            Ok(blueprint) => match validate_blueprint(&blueprint, samples) {
                Ok(()) => return Ok(blueprint),
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error,
        }
    }
    Err(AppError::UnprocessableEntity(format!(
        "model did not return a valid course blueprint: {last_error}"
    )))
}


pub(crate) fn build_blueprint_prompt(
    name: &str,
    description: &str,
    domain: Option<&str>,
    samples: &[(String, String)],
) -> String {
    let mut prompt = format!(
        "Knowledge base name: {}\nKnowledge base description: {}\n\
         Course size is yours to decide: choose the number of modules and lessons \
         per module from the scope and complexity of the sampled material.\n",
        name.trim(),
        description.trim()
    );
    if let Some(domain) = domain.map(str::trim).filter(|domain| !domain.is_empty()) {
        prompt.push_str(&format!("Requested domain label: {domain}\n"));
    }
    prompt.push_str(&format!("Sampled documents ({}):\n", samples.len()));
    for (path, excerpt) in samples {
        prompt.push_str(&format!("\n--- FILE: {path} ---\n{excerpt}\n"));
    }
    prompt.push_str("\nDesign the course blueprint JSON now.");
    prompt
}


/// Appended to the blueprint system prompt when no sampled documents exist
/// (description flow): the brief is the whole grounding and the `source`
/// field is omitted.
const BLUEPRINT_NO_SAMPLE_RULE: &str = "No sampled documents are provided for this run: ground \
every lesson in the course brief itself and omit the \"source\" field from every lesson.";


/// Description-flow variant of [`build_blueprint_prompt`]: the brief is the
/// whole grounding — no knowledge base, no samples, no source citations.
pub(crate) fn build_description_blueprint_prompt(
    description: &str,
    domain: Option<&str>,
) -> String {
    let mut prompt = format!(
        "Course brief:\n{}\n\
         Course size is yours to decide: choose the number of modules and lessons \
         per module from the brief's scope and complexity.\n",
        description.trim()
    );
    if let Some(domain) = domain.map(str::trim).filter(|domain| !domain.is_empty()) {
        prompt.push_str(&format!("Requested domain label: {domain}\n"));
    }
    prompt.push_str("\nDesign the course blueprint JSON now.");
    prompt
}


pub(crate) fn validate_blueprint(
    blueprint: &Blueprint,
    samples: &[(String, String)],
) -> Result<(), String> {
    if blueprint.title.trim().is_empty() {
        return Err("blueprint title is empty".into());
    }
    if blueprint.modules.is_empty() {
        return Err("blueprint has no modules".into());
    }
    let source_paths: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
    for module in &blueprint.modules {
        if module.title.trim().is_empty() || module.lessons.is_empty() {
            return Err("each module needs a title and at least one lesson".into());
        }
        for lesson in &module.lessons {
            if lesson.title.trim().is_empty() {
                return Err("lesson title is required".into());
            }
            // kb flow: every lesson must cite an exact sampled file. The
            // description flow has no samples, so no source is required and
            // an invented path is simply dropped on import.
            if !samples.is_empty() {
                let Some(source) = &lesson.source else {
                    return Err(format!("lesson \"{}\" has no source", lesson.title));
                };
                if !source_paths.contains(source.path.as_str()) {
                    return Err(format!(
                        "lesson \"{}\" cites an unsampled source path: {}",
                        lesson.title, source.path
                    ));
                }
            }
        }
    }
    Ok(())
}

