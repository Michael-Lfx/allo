//! Deck creation with an honest capability probe.
//!
//! The generator is loaded from the environment: the built-in renderer is used
//! when present, and an operator or OS image can declare it unavailable
//! (`FLOWY_PPTX=missing`). A missing generator is reported as `status: missing`
//! and is not a failed turn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use flowy_agent_core::ToolCallArgs;
use flowy_agent_core::ToolExecutionResult;
use flowy_agent_core::tools::ToolDefinition;
use rpptx::Presentation;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckArgs {
    path: String,
    title: String,
    slides: Vec<String>,
}

/// Whether a deck generator is available in this environment.
///
/// The built-in renderer ships with the binary; an environment that removes it
/// (an image without the optional asset, or an operator setting the switch)
/// must be able to say so without turning the turn into a failure.
fn generator_available() -> bool {
    !matches!(std::env::var("FLOWY_PPTX").ok().as_deref(), Some("missing"))
}

fn resolve_path(cwd: Option<&str>, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match cwd {
        Some(cwd) => Path::new(cwd).join(path),
        None => path.to_path_buf(),
    }
}

fn missing_result() -> ToolExecutionResult {
    ToolExecutionResult::success_text(
        "missing: no pptx generator is available in this environment",
        json!({
            "status": "missing",
            "need": "pptx generator",
            "hint": "install a deck skill or set FLOWY_PPTX=missing to declare it unavailable",
        }),
    )
}

fn write_deck(
    path: &Path,
    title: &str,
    slides: &[String],
) -> Result<usize, String> {
    let mut presentation =
        Presentation::new().map_err(|error| error.to_string())?;
    for (index, body) in slides.iter().enumerate() {
        let slide_index = presentation.slides().len();
        presentation
            .add_slide(0)
            .map_err(|error| error.to_string())?;
        let mut slide = presentation
            .slide_mut(slide_index)
            .ok_or_else(|| "the new slide disappeared".to_owned())?;
        let heading = if index == 0 {
            title.to_owned()
        } else {
            format!("{title} · {}", index + 1)
        };
        slide
            .add_textbox(
                rpptx::Emu::from_inches(0.6),
                rpptx::Emu::from_inches(0.4),
                rpptx::Emu::from_inches(9.0),
                rpptx::Emu::from_inches(0.9),
            )
            .map_err(|error| error.to_string())?
            .set_text(&heading)
            .map_err(|error| error.to_string())?;
        if !body.trim().is_empty() {
            slide
                .add_textbox(
                    rpptx::Emu::from_inches(0.6),
                    rpptx::Emu::from_inches(1.6),
                    rpptx::Emu::from_inches(9.0),
                    rpptx::Emu::from_inches(4.6),
                )
                .map_err(|error| error.to_string())?
                .set_text(body)
                .map_err(|error| error.to_string())?;
        }
    }
    if slides.is_empty() {
        return Err("a deck needs at least one slide".to_owned());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    presentation.save(path).map_err(|error| error.to_string())?;
    Ok(slides.len())
}

/// Shared execution path for tests and the tool definition.
fn execute(
    available: bool,
    arguments: Value,
    cwd: Option<&str>,
) -> ToolExecutionResult {
    if !available {
        return missing_result();
    }
    let Ok(args) = serde_json::from_value::<DeckArgs>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path, title and a non-empty slides array",
        );
    };
    let path = resolve_path(cwd, &args.path);
    match write_deck(&path, &args.title, &args.slides) {
        Ok(count) => ToolExecutionResult::success_text(
            format!("created a {count}-slide deck at {}", path.display()),
            json!({
                "status": "ready",
                "path": path.display().to_string(),
                "slides": count,
            }),
        ),
        Err(error) => ToolExecutionResult::error(
            "PPTX_FAILED",
            format!("could not write {}: {error}", path.display()),
        ),
    }
}

pub fn definition() -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "create_deck",
        "Create a PowerPoint (.pptx) deck in one call. Use this when the user asks for slides or a deck. The generator is provided by the environment; a missing generator is reported as status=missing, not as a failure.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1, "description": "Output path, relative to the working directory."},
                "title": {"type": "string", "minLength": 1, "description": "Deck title, repeated on every slide."},
                "slides": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 12,
                    "items": {"type": "string"},
                    "description": "Body text for each slide, in order."
                }
            },
            "required": ["path", "title", "slides"],
            "additionalProperties": false
        }),
        |args: ToolCallArgs, context: flowy_agent_core::ToolRunContext| {
            Box::pin(async move {
                execute(
                    generator_available(),
                    args.arguments,
                    context.cwd.as_deref(),
                )
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir()
            .join(format!("flowy-deck-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn writing_a_three_slide_deck_produces_three_slides() {
        let root = scratch("write");
        let path = root.join("deck.pptx");
        let arguments = json!({
            "path": "deck.pptx",
            "title": "Quarterly review",
            "slides": ["one", "two", "three"],
        });
        let outcome = execute(true, arguments, Some(root.to_str().unwrap()));
        let ToolExecutionResult::Success { value, .. } = outcome else {
            panic!("expected a created deck, got {outcome:?}");
        };
        assert_eq!(value["status"], "ready");
        assert_eq!(value["slides"], 3);
        let bytes = std::fs::read(&path).expect("the deck is on disk");
        assert_eq!(&bytes[..2], b"PK", "a pptx is a zip container");
        let presentation =
            Presentation::from_bytes(&bytes).expect("the deck reads back");
        assert_eq!(presentation.slides().len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_generator_is_reported_as_missing_not_failure() {
        let outcome = execute(
            false,
            json!({"path": "deck.pptx", "title": "T", "slides": ["one"]}),
            None,
        );
        let ToolExecutionResult::Success { value, .. } = outcome else {
            panic!("missing must not be a tool error");
        };
        assert_eq!(value["status"], "missing");
        assert_eq!(value["need"], "pptx generator");
    }
}
