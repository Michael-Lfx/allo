use std::sync::Arc;

use regex::Regex;
use serde::Deserialize;

use crate::backends::VimaxChat;
use crate::clip_bounds::ClipBounds;
use crate::drama::{DramaEngine, lint_drama_engine};
use crate::error::{VimaxError, VimaxResult};
use crate::json_util::{LLM_JSON_PARSE_ATTEMPTS, complete_and_parse_llm_json, parse_llm_json};

use super::formats::{DRAMA_ENGINE, SCRIPT_SCENES};

pub struct Screenwriter {
    chat: Arc<dyn VimaxChat>,
    /// Clip window of the session's video model: a line has to be speakable
    /// inside one clip, so the writer must know how long a clip can be.
    clip: ClipBounds,
}

impl Screenwriter {
    pub fn new(chat: Arc<dyn VimaxChat>, clip: ClipBounds) -> Self {
        Self { chat, clip }
    }

    /// Generate the film's dramatic engine and validate it with the
    /// deterministic lints in [`crate::drama`]: generate → lint → one repair
    /// round with the concrete defect list → lint again. A still-failing
    /// engine is a hard error — spending video credits on a story with no
    /// want/obstacle/reversal is exactly the "戏太瘦" failure this prevents.
    pub async fn develop_validated_drama_engine(
        &self,
        idea: &str,
        user_requirement: &str,
    ) -> VimaxResult<DramaEngine> {
        let system = include_str!(
            "../../prompts/screenwriter__system_prompt_template_develop_drama_engine.txt"
        )
        .replace("{format_instructions}", DRAMA_ENGINE);
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_develop_drama_engine.txt"
        )
        .replace("{idea}", idea)
        .replace("{user_requirement}", user_requirement);

        let engine =
            complete_and_parse_llm_json::<DramaEngine>(self.chat.as_ref(), &system, &user).await?;
        let issues = lint_drama_engine(&engine);
        if issues.is_empty() {
            return Ok(engine);
        }

        tracing::info!(
            issues = issues.len(),
            "drama engine failed lint; running one repair round"
        );
        let repair_user = format!(
            "{user}\n\n[LINT_FEEDBACK]\nYour previous engine was:\n{}\n\nIt has these defects — fix ALL of them and keep everything that was not flagged:\n- {}",
            serde_json::to_string_pretty(&engine).unwrap_or_default(),
            issues.join("\n- ")
        );
        let repaired =
            complete_and_parse_llm_json::<DramaEngine>(self.chat.as_ref(), &system, &repair_user)
                .await?;
        let remaining = lint_drama_engine(&repaired);
        if remaining.is_empty() {
            return Ok(repaired);
        }
        Err(VimaxError::Llm(format!(
            "drama engine still fails lint after repair: {}",
            remaining.join("; ")
        )))
    }

    pub async fn develop_story(
        &self,
        idea: &str,
        user_requirement: &str,
    ) -> VimaxResult<String> {
        let system =
            include_str!("../../prompts/screenwriter__system_prompt_template_develop_story.txt")
                .replace("{clip_max_secs}", &self.clip.max_secs().to_string());
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_develop_story.txt"
        )
        .replace("{idea}", idea)
        .replace("{user_requirement}", user_requirement);
        self.chat.complete_text(&system, &user).await
    }

    pub async fn write_script_based_on_story(
        &self,
        story: &str,
        user_requirement: &str,
    ) -> VimaxResult<Vec<String>> {
        let system = include_str!(
            "../../prompts/screenwriter__system_prompt_template_write_script_based_on_story.txt"
        )
        .replace("{format_instructions}", SCRIPT_SCENES)
        .replace("{clip_max_secs}", &self.clip.max_secs().to_string());
        // Prompt file may not have format_instructions — inject into system if absent.
        let system = if system.contains("{format_instructions}") {
            system
        } else {
            format!("{system}\n\n[Output]\n{SCRIPT_SCENES}")
        };
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_write_script_based_on_story.txt"
        )
        .replace("{story}", story)
        .replace("{user_requirement}", user_requirement);

        // Prefer structured JSON (with shared retry/repair). If the model still
        // emits broken screenplay JSON, salvage scenes from the last raw reply.
        match complete_and_parse_llm_json::<ScenesOut>(self.chat.as_ref(), &system, &user).await {
            Ok(out) => return Ok(out.into_scenes()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "screenwriter JSON parse failed after retries; attempting lenient salvage"
                );
            }
        }

        let salvage_user = format!(
            "{user}\n\n\
---\n\
FINAL FALLBACK ({LLM_JSON_PARSE_ATTEMPTS}+): Return either valid JSON \
{{\"scenes\":[\"...\"]}} OR plain screenplay text with clear scene headings \
like「场景一」「场景二」/「Scene 1」. Prefer 「」 quotes inside dialogue."
        );
        let raw = self.chat.complete_text(&system, &salvage_user).await?;
        if let Ok(out) = parse_llm_json::<ScenesOut>(&raw) {
            return Ok(out.into_scenes());
        }
        if let Some(scenes) = extract_scenes_lenient(&raw) {
            tracing::info!(
                count = scenes.len(),
                "screenwriter salvaged scenes via lenient extractor"
            );
            return Ok(scenes);
        }
        Err(VimaxError::Llm(format!(
            "failed to parse screenwriter scenes JSON and lenient salvage found no scenes; body={}",
            &raw.chars().take(300).collect::<String>()
        )))
    }

    /// Best-effort scene polish: action lines that only NAME an emotion, and
    /// scenes with no audible plot (no named 「」 dialogue). One rewrite round.
    /// Infallible by design — residual lint downgrades to warnings, never
    /// errors. A repair that changes the scene count is discarded.
    pub async fn polish_scenes_show_dont_tell(&self, scenes: Vec<String>) -> Vec<String> {
        let issues = crate::drama::lint_scenes(&scenes);
        if issues.is_empty() {
            return scenes;
        }
        tracing::info!(
            issues = issues.len(),
            "scene scripts failed spoken-plot/show-don't-tell lint; running one polish round"
        );
        let scenes_json = match serde_json::to_string_pretty(&scenes) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "cannot serialize scenes for polish; keeping original");
                return scenes;
            }
        };
        let system = include_str!(
            "../../prompts/screenwriter__system_prompt_template_polish_scenes.txt"
        )
        .replace("{format_instructions}", SCRIPT_SCENES);
        let user = include_str!(
            "../../prompts/screenwriter__human_prompt_template_polish_scenes.txt"
        )
        .replace("{scenes_json}", &scenes_json)
        .replace("{lint_feedback}", &format!("- {}", issues.join("\n- ")));

        match complete_and_parse_llm_json::<ScenesOut>(self.chat.as_ref(), &system, &user).await {
            Ok(out) => {
                let polished = out.into_scenes();
                if polished.len() != scenes.len() {
                    tracing::warn!(
                        original = scenes.len(),
                        polished = polished.len(),
                        "scene polish changed scene count; discarding polish"
                    );
                    return scenes;
                }
                let remaining = crate::drama::lint_scenes(&polished);
                if !remaining.is_empty() {
                    tracing::warn!(
                        remaining = remaining.len(),
                        "show-don't-tell lint still failing after polish; proceeding anyway"
                    );
                }
                polished
            }
            Err(e) => {
                tracing::warn!(error = %e, "scene polish round failed; keeping original scenes");
                scenes
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScenesOut {
    Arr(Vec<String>),
    Obj { scenes: Vec<String> },
}

impl ScenesOut {
    fn into_scenes(self) -> Vec<String> {
        match self {
            ScenesOut::Arr(scenes) => scenes,
            ScenesOut::Obj { scenes } => scenes,
        }
    }
}

/// Recover scene scripts when JSON is too broken to parse.
/// Accepts: repaired JSON, fenced blocks, or plain text with scene headings.
pub(crate) fn extract_scenes_lenient(raw: &str) -> Option<Vec<String>> {
    if let Ok(out) = parse_llm_json::<ScenesOut>(raw) {
        let scenes = out.into_scenes();
        if !scenes.is_empty() {
            return Some(scenes);
        }
    }

    let text = strip_code_fence(raw.trim());
    if text.is_empty() {
        return None;
    }

    // Split on common Chinese / English scene headings at line starts.
    static HEADING: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let heading = HEADING.get_or_init(|| {
        Regex::new(
            r"(?m)^(?:#{1,3}\s*)?(?:场景\s*[一二三四五六七八九十百零〇\d]+|Scene\s*\d+|SCENE\s*\d+)[^\n]*",
        )
        .expect("scene heading regex")
    });

    let mut starts: Vec<usize> = heading.find_iter(&text).map(|m| m.start()).collect();
    if starts.is_empty() {
        // Single blob still usable as one scene if it looks like a screenplay.
        if looks_like_screenplay(&text) {
            return Some(vec![text]);
        }
        return None;
    }
    starts.push(text.len());
    let mut scenes = Vec::new();
    for w in starts.windows(2) {
        let chunk = text[w[0]..w[1]].trim();
        if !chunk.is_empty() {
            scenes.push(chunk.to_string());
        }
    }
    if scenes.is_empty() {
        None
    } else {
        Some(scenes)
    }
}

fn strip_code_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("JSON"))
            .unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
        return rest.trim().to_string();
    }
    t.to_string()
}

fn looks_like_screenplay(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    s.contains('△')
        || s.contains("内景")
        || s.contains("外景")
        || s.contains("INT.")
        || s.contains("EXT.")
        || lower.contains("scene")
        || s.contains('：')
        || (s.contains(':') && s.chars().count() > 80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_split_on_chinese_scene_headings() {
        let raw = r#"
场景一:县衙大堂
△ 马县长坐着。
马县长:黄老爷!

场景二:黄府书房
△ 黄四郎喝茶。
"#;
        let scenes = extract_scenes_lenient(raw).expect("scenes");
        assert_eq!(scenes.len(), 2);
        assert!(scenes[0].contains("县衙"));
        assert!(scenes[1].contains("黄府"));
    }

    #[test]
    fn lenient_recovers_broken_json_with_inner_quotes() {
        let raw = r#"{
  "scenes": [
    "场景一:大堂\n△ 发出"咚"的一声。\n马县长:哼!"
  ]
}"#;
        let scenes = extract_scenes_lenient(raw).expect("scenes");
        assert_eq!(scenes.len(), 1);
        assert!(scenes[0].contains("咚"));
    }
}
