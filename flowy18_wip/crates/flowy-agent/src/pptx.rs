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
use flowy_agent_core::tools::{ToolDefinition, ToolSource};
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

/// Maximum slide count enforced at runtime (matches tool JSON schema).
const MAX_DECK_SLIDES: usize = 12;
/// Maximum combined UTF-8 byte length across title + all slide bodies (512 KiB).
const MAX_DECK_TEXT_BYTES: usize = 512 * 1024;

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn resolve_turn_path(
    cwd: Option<&str>,
    output_root: Option<&Path>,
    raw: &str,
) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(output_root) = output_root {
        if let Some(cwd) = cwd {
            let from_cwd = Path::new(cwd).join(path);
            if from_cwd.starts_with(output_root) {
                return from_cwd;
            }
        }
        return output_root.join(path);
    }
    match cwd {
        Some(cwd) => Path::new(cwd).join(path),
        None => path.to_path_buf(),
    }
}

/// Resolve a tool path relative to `output_root` (or `cwd`) AND re-verify
/// workspace confinement against the actual resolved path so a symlink/junction
/// inside `output_root` cannot bypass `path_guard`'s `cwd`-relative check.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn resolve_confined_turn_path(
    cwd: Option<&str>,
    output_root: Option<&Path>,
    raw: &str,
) -> Result<PathBuf, String> {
    if raw.trim().is_empty() || raw.contains('\0') {
        return Err("path must not be empty".to_owned());
    }
    let confinement_root = cwd
        .map(Path::new)
        .or(output_root)
        .ok_or_else(|| "working directory unavailable".to_owned())?;
    let resolved = resolve_turn_path(cwd, output_root, raw);
    let resolved_str = resolved.to_string_lossy();
    if !crate::handle::confined_to(confinement_root, &resolved_str) {
        return Err(format!("path escapes the working directory: {raw}"));
    }
    if let Some(out_root) = output_root
        && !Path::new(raw).is_absolute()
        && !crate::handle::confined_to(out_root, &resolved_str)
    {
        return Err(format!("path escapes the turn output directory: {raw}"));
    }
    Ok(resolved)
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

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn write_deck(
    path: &Path,
    title: &str,
    slides: &[String],
) -> Result<usize, String> {
    if slides.is_empty() {
        return Err("a deck needs at least one slide".to_owned());
    }
    if slides.len() > MAX_DECK_SLIDES {
        return Err(format!(
            "a deck may have at most {MAX_DECK_SLIDES} slides (got {})",
            slides.len()
        ));
    }
    let total_text: usize = title
        .len()
        .saturating_add(slides.iter().map(String::len).sum::<usize>());
    if total_text > MAX_DECK_TEXT_BYTES {
        return Err(format!(
            "deck text exceeds {MAX_DECK_TEXT_BYTES}-byte limit"
        ));
    }
    if crate::handle::has_reserved_device_component(path) {
        return Err("reserved device path refused".to_owned());
    }
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && (!meta.is_file()
            || meta.file_type().is_symlink()
            || flowy_agent_core::tools::is_windows_reparse_point(&meta))
    {
        return Err(
            "refusing to overwrite non-regular or linked file".to_owned()
        );
    }
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
        let mut title_box = slide
            .add_textbox(
                rpptx::Emu::from_inches(0.6),
                rpptx::Emu::from_inches(0.4),
                rpptx::Emu::from_inches(9.0),
                rpptx::Emu::from_inches(0.9),
            )
            .map_err(|error| error.to_string())?;
        title_box
            .set_name(&format!("S{index}-title"))
            .map_err(|error| error.to_string())?;
        title_box
            .set_text(&heading)
            .map_err(|error| error.to_string())?;
        if !body.trim().is_empty() {
            let mut body_box = slide
                .add_textbox(
                    rpptx::Emu::from_inches(0.6),
                    rpptx::Emu::from_inches(1.6),
                    rpptx::Emu::from_inches(9.0),
                    rpptx::Emu::from_inches(4.6),
                )
                .map_err(|error| error.to_string())?;
            body_box
                .set_name(&format!("S{index}-body"))
                .map_err(|error| error.to_string())?;
            body_box.set_text(body).map_err(|error| error.to_string())?;
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
    output_root: Option<&Path>,
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
    let path = match resolve_confined_turn_path(cwd, output_root, &args.path) {
        Ok(path) => path,
        Err(error) => {
            return ToolExecutionResult::error("INVALID_PATH", error);
        }
    };
    match write_deck(&path, &args.title, &args.slides) {
        Ok(count) => {
            let mut anchors = Vec::with_capacity(count * 2);
            for (index, body) in args.slides.iter().enumerate() {
                anchors.push(format!("S{index}-title"));
                if !body.trim().is_empty() {
                    anchors.push(format!("S{index}-body"));
                }
            }
            let anchor_list = anchors.join(", ");
            ToolExecutionResult::success_text(
                format!(
                    "created a {count}-slide deck at {} (shape anchors: {anchor_list})",
                    path.display()
                ),
                json!({
                    "status": "ready",
                    "path": path.display().to_string(),
                    "slides": count,
                    "anchors": anchors,
                }),
            )
        }
        Err(error) => ToolExecutionResult::error(
            "PPTX_FAILED",
            format!("could not write {}: {error}", path.display()),
        ),
    }
}

pub fn definition() -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "create_deck",
        "Create a PowerPoint (.pptx) deck in one call. Use this when the user asks for slides or a deck. Each 0-based slide index i creates named shape anchors S{i}-title (slide 0 has title, slide i>0 has '{title} · {i+1}') and S{i}-body (for non-empty body text). The generator is provided by the environment; a missing generator is reported as status=missing, not as a failure.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1, "description": "Output path, relative to the current turn output directory (or working directory)."},
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
                let available = generator_available();
                let arguments = args.arguments;
                let cwd = context.cwd;
                let output_root = context.output_root;
                tokio::task::spawn_blocking(move || {
                    execute(
                        available,
                        arguments,
                        cwd.as_deref(),
                        output_root.as_deref(),
                    )
                })
                .await
                .unwrap_or_else(|error| {
                    ToolExecutionResult::error(
                        "PPTX_FAILED",
                        format!("deck generator task failed: {error}"),
                    )
                })
            })
        },
    )
    .with_source(ToolSource::Host))
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
        let outcome =
            execute(true, arguments, Some(root.to_str().unwrap()), None);
        let ToolExecutionResult::Success { value, .. } = outcome else {
            panic!("expected a created deck, got {outcome:?}");
        };
        assert_eq!(value["status"], "ready");
        assert_eq!(value["slides"], 3);
        assert_eq!(
            value["anchors"],
            json!([
                "S0-title", "S0-body", "S1-title", "S1-body", "S2-title",
                "S2-body"
            ])
        );
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
            None,
        );
        let ToolExecutionResult::Success { value, .. } = outcome else {
            panic!("missing must not be a tool error");
        };
        assert_eq!(value["status"], "missing");
        assert_eq!(value["need"], "pptx generator");
    }

    #[test]
    fn turn_output_root_resolves_relative_and_cwd_prefixed_paths_consistently()
    {
        let root = scratch("output-root");
        let output = root.join("outputs").join("submission-1");
        let cwd = root.to_str().unwrap();
        assert_eq!(
            resolve_turn_path(Some(cwd), Some(&output), "deck.pptx"),
            output.join("deck.pptx")
        );
        assert_eq!(
            resolve_turn_path(
                Some(cwd),
                Some(&output),
                "outputs/submission-1/deck.pptx"
            ),
            root.join("outputs/submission-1/deck.pptx")
        );
        // Reject Windows ADS / device names and >12 slide decks at runtime.
        assert!(
            resolve_confined_turn_path(Some(cwd), Some(&output), "CON:a")
                .is_err()
        );
        assert!(
            resolve_confined_turn_path(
                Some(cwd),
                Some(&output),
                "../escape.pptx"
            )
            .is_err()
        );
        let too_many = execute(
            true,
            json!({
                "path": "big.pptx",
                "title": "Big",
                "slides": vec!["body"; 13],
            }),
            Some(cwd),
            Some(&output),
        );
        assert!(matches!(too_many, ToolExecutionResult::Error { .. }));
        let _ = std::fs::remove_dir_all(&root);
    }
}
