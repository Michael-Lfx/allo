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
    module_count: u8,
    lessons_per_module: u8,
) -> Result<Blueprint, AppError> {
    let mut last_error = String::new();
    for attempt in 0..2 {
        let user = if attempt == 0 {
            prompt.to_owned()
        } else {
            format!(
                "{prompt}\n\nThe previous blueprint was rejected: {last_error}\n\
                 Return a corrected blueprint JSON now, keeping the requested size."
            )
        };
        let raw = complete(
            completer,
            model_override,
            BLUEPRINT_SYSTEM,
            &user,
            BLUEPRINT_MAX_TOKENS,
        )
        .await?;
        match parse_json_object::<Blueprint>(&raw) {
            Ok(blueprint) => match validate_blueprint(&blueprint, samples, module_count, lessons_per_module) {
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
    module_count: u8,
    lessons_per_module: u8,
    samples: &[(String, String)],
) -> String {
    let mut prompt = format!(
        "Knowledge base name: {}\nKnowledge base description: {}\n\
         Target size: exactly {module_count} modules and {lessons_per_module} lessons per module.\n",
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


pub(super) fn validate_blueprint(
    blueprint: &Blueprint,
    samples: &[(String, String)],
    module_count: u8,
    lessons_per_module: u8,
) -> Result<(), String> {
    if blueprint.title.trim().is_empty() {
        return Err("blueprint title is empty".into());
    }
    if blueprint.modules.is_empty() {
        return Err("blueprint has no modules".into());
    }
    if blueprint.modules.len() != module_count as usize {
        return Err(format!(
            "blueprint has {} modules, expected {module_count}",
            blueprint.modules.len()
        ));
    }
    let mut concept_keys = HashSet::new();
    for concept in &blueprint.concepts {
        let key = concept.key.trim();
        if key.is_empty() || concept.title.trim().is_empty() {
            return Err("concept key and title are required".into());
        }
        if !concept_keys.insert(key.to_owned()) {
            return Err(format!("duplicate concept key: {key}"));
        }
        for prerequisite in &concept.prerequisites {
            if !concept_keys.contains(prerequisite.trim()) {
                return Err(format!(
                    "concept {key} references unknown prerequisite {prerequisite}"
                ));
            }
            if prerequisite == key {
                return Err(format!("concept {key} cannot require itself"));
            }
        }
    }
    if blueprint_concept_cycle(&blueprint.concepts) {
        return Err("concept prerequisites form a cycle".into());
    }
    let source_paths: HashSet<&str> = samples.iter().map(|(path, _)| path.as_str()).collect();
    let mut lesson_count = 0usize;
    for module in &blueprint.modules {
        if module.title.trim().is_empty() || module.lessons.is_empty() {
            return Err("each module needs a title and at least one lesson".into());
        }
        if module.lessons.len() != lessons_per_module as usize {
            return Err(format!(
                "module \"{}\" has {} lessons, expected {lessons_per_module}",
                module.title,
                module.lessons.len()
            ));
        }
        for lesson in &module.lessons {
            lesson_count += 1;
            if lesson.title.trim().is_empty() {
                return Err("lesson title is required".into());
            }
            if lesson.concepts.is_empty() {
                return Err(format!("lesson \"{}\" binds no concept", lesson.title));
            }
            for concept in &lesson.concepts {
                if !concept_keys.contains(concept.trim()) {
                    return Err(format!(
                        "lesson \"{}\" references unknown concept {concept}",
                        lesson.title
                    ));
                }
            }
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
    if lesson_count != module_count as usize * lessons_per_module as usize {
        return Err(format!(
            "blueprint has {lesson_count} lessons, expected {}",
            module_count as usize * lessons_per_module as usize
        ));
    }
    Ok(())
}


/// Cycle detection over the prerequisite graph (Kahn's algorithm: repeatedly
/// strip concepts whose prerequisites are all gone; leftovers form a cycle).
fn blueprint_concept_cycle(concepts: &[ConceptPack]) -> bool {
    let mut remaining: HashSet<&str> = concepts.iter().map(|c| c.key.as_str()).collect();
    loop {
        let ready: Vec<&str> = concepts
            .iter()
            .filter(|concept| {
                remaining.contains(concept.key.as_str())
                    && concept
                        .prerequisites
                        .iter()
                        .all(|prerequisite| !remaining.contains(prerequisite.as_str()))
            })
            .map(|concept| concept.key.as_str())
            .collect();
        if ready.is_empty() {
            break;
        }
        for key in ready {
            remaining.remove(key);
        }
    }
    !remaining.is_empty()
}

