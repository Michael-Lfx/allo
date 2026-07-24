use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backends::{VimaxChat, VimaxImage};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;
use crate::session::{read_json_artifact, write_json_artifact};

use super::formats::WORLD_ASSETS;

/// Extracted environment plate for global consistency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentAsset {
    pub idx: i32,
    pub slugline: String,
    pub description: String,
}

/// Key prop / object that must stay consistent across shots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropAsset {
    pub idx: i32,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldAssetsSpec {
    #[serde(default)]
    pub environments: Vec<EnvironmentAsset>,
    #[serde(default)]
    pub props: Vec<PropAsset>,
}

/// Registry consumed by frame generation: name → {path, description}.
pub type WorldAssetRegistry = HashMap<String, HashMap<String, HashMap<String, String>>>;

pub struct WorldAssetsPlanner {
    chat: Arc<dyn VimaxChat>,
    image: Arc<dyn VimaxImage>,
}

impl WorldAssetsPlanner {
    pub fn new(chat: Arc<dyn VimaxChat>, image: Arc<dyn VimaxImage>) -> Self {
        Self { chat, image }
    }

    pub async fn extract(&self, script_or_story: &str, style: &str) -> VimaxResult<WorldAssetsSpec> {
        let style = crate::planning::resolve_visual_style(style);
        let system = include_str!(
            "../../prompts/world_assets__system_prompt_template_extract.txt"
        )
        .replace("{format_instructions}", WORLD_ASSETS);
        let user = include_str!("../../prompts/world_assets__human_prompt_template_extract.txt")
            .replace("{style}", &style)
            .replace("{text}", script_or_story);
        let raw = self.chat.complete_text(&system, &user).await?;
        let mut spec: WorldAssetsSpec = parse_llm_json(&raw)?;
        if spec.environments.len() > 5 {
            spec.environments.truncate(5);
        }
        if spec.props.len() > 8 {
            spec.props.truncate(8);
        }
        for (i, e) in spec.environments.iter_mut().enumerate() {
            e.idx = i as i32;
            e.description = strip_people_mentions(&e.description);
        }
        for (i, p) in spec.props.iter_mut().enumerate() {
            p.idx = i as i32;
            p.description = strip_people_mentions(&p.description);
        }
        Ok(spec)
    }

    /// Extract (if needed) and generate missing environment / prop plates under `film_root`.
    pub async fn ensure(
        &self,
        film_root: &Path,
        script_or_story: &str,
        style: &str,
    ) -> VimaxResult<WorldAssetRegistry> {
        tokio::fs::create_dir_all(film_root).await?;
        let spec_path = film_root.join("world_assets.json");
        let registry_path = film_root.join("world_assets_registry.json");

        let style = crate::planning::resolve_visual_style(style);
        let theme = theme_excerpt(script_or_story);

        let spec: WorldAssetsSpec = if spec_path.exists() {
            read_json_artifact(&spec_path).await?
        } else {
            let spec = self.extract(script_or_story, &style).await?;
            write_json_artifact(&spec_path, &spec).await?;
            spec
        };

        let mut registry: WorldAssetRegistry = if registry_path.exists() {
            read_json_artifact(&registry_path).await?
        } else {
            HashMap::new()
        };

        let env_root = film_root.join("environments");
        let prop_root = film_root.join("props");
        tokio::fs::create_dir_all(&env_root).await?;
        tokio::fs::create_dir_all(&prop_root).await?;

        let mut env_map = registry.remove("environments").unwrap_or_default();
        for env in &spec.environments {
            let key = if env.slugline.trim().is_empty() {
                format!("env_{}", env.idx)
            } else {
                env.slugline.trim().to_string()
            };
            if env_map.contains_key(&key) {
                continue;
            }
            let dir = env_root.join(format!("{}_{}", env.idx, safe_component(&key)));
            tokio::fs::create_dir_all(&dir).await?;
            let out = dir.join("plate.png");
            if !out.exists() {
                let desc = strip_people_mentions(&env.description);
                let style_clause = crate::planning::style_prompt_clause(&style);
                let prompt = include_str!(
                    "../../prompts/world_assets__prompt_template_environment_plate.txt"
                )
                .replace("{theme}", &theme)
                .replace("{slugline}", &env.slugline)
                .replace("{description}", &desc)
                .replace("{style}", &style_clause);
                self.generate_empty_plate(&prompt, &out).await?;
            }
            let detail: String = strip_people_mentions(&env.description)
                .chars()
                .take(120)
                .collect();
            env_map.insert(
                key.clone(),
                asset_item(
                    &out,
                    &format!(
                        "GLOBAL EMPTY environment plate (no people): {key}. {detail}. Lock architecture, lighting, set dressing only."
                    ),
                ),
            );
        }

        let mut prop_map = registry.remove("props").unwrap_or_default();
        for prop in &spec.props {
            let key = prop.name.trim().to_string();
            if key.is_empty() || prop_map.contains_key(&key) {
                continue;
            }
            let dir = prop_root.join(format!("{}_{}", prop.idx, safe_component(&key)));
            tokio::fs::create_dir_all(&dir).await?;
            let out = dir.join("prop.png");
            if !out.exists() {
                let desc = strip_people_mentions(&prop.description);
                let style_clause = crate::planning::style_prompt_clause(&style);
                let prompt = include_str!("../../prompts/world_assets__prompt_template_prop.txt")
                    .replace("{theme}", &theme)
                    .replace("{name}", &prop.name)
                    .replace("{description}", &desc)
                    .replace("{style}", &style_clause);
                self.generate_empty_plate(&prompt, &out).await?;
            }
            let detail: String = strip_people_mentions(&prop.description)
                .chars()
                .take(100)
                .collect();
            prop_map.insert(
                key.clone(),
                asset_item(
                    &out,
                    &format!(
                        "GLOBAL prop bible (object only, no people): <{key}>. {detail}. Lock shape, materials, colors."
                    ),
                ),
            );
        }

        registry.insert("environments".into(), env_map);
        registry.insert("props".into(), prop_map);
        write_json_artifact(&registry_path, &registry).await?;
        Ok(registry)
    }

    /// Generate vacant plate; if vision sees people, retry with stronger empty-set prompts.
    async fn generate_empty_plate(&self, prompt: &str, out: &Path) -> VimaxResult<()> {
        self.image.generate(prompt, &[], out).await?;
        if !self.plate_has_people(out).await {
            return Ok(());
        }

        for attempt in 1..=2 {
            tracing::warn!(
                path = %out.display(),
                attempt,
                "world asset plate contains people; regenerating empty-set"
            );
            let _ = tokio::fs::remove_file(out).await;
            let retry = if attempt == 1 {
                format!(
                    "{prompt}\nCRITICAL RETRY: previous image illegally showed humans. \
                     Vacant plate only — ZERO people, ZERO faces, ZERO silhouettes, ZERO hands."
                )
            } else {
                // Short hard prompt so safety prefix + truncate cannot bury the empty-set rule.
                format!(
                    "Wide 16:9 vacant unoccupied film location or isolated object plate. \
                     Completely empty. Zero people, zero humans, zero faces, zero silhouettes, zero hands, zero body parts. \
                     Architecture furniture props lighting only. {prompt}"
                )
            };
            self.image.generate(&retry, &[], out).await?;
            if !self.plate_has_people(out).await {
                return Ok(());
            }
        }

        // Do not keep a contaminated plate in the registry path — force caller to notice.
        tracing::error!(
            path = %out.display(),
            "world asset plate still contains people after retries; deleting bad file"
        );
        let _ = tokio::fs::remove_file(out).await;
        Err(crate::error::VimaxError::Image(format!(
            "empty-set plate still contains people after retries: {}",
            out.display()
        )))
    }

    async fn plate_has_people(&self, path: &Path) -> bool {
        let raw = match self
            .chat
            .complete_vision(
                "You are a strict image inspector. Reply with exactly YES or NO.",
                "Does this image contain any human, person, face, crowd, silhouette of a person, hand, or body part? YES or NO only.",
                &[path],
            )
            .await
        {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!(error = %err, "world-asset people check failed; assuming clean");
                return false;
            }
        };
        let upper = raw.trim().to_ascii_uppercase();
        let trimmed = raw.trim();
        if upper.starts_with("NO")
            || trimmed.starts_with('否')
            || trimmed.starts_with("没有")
            || trimmed.starts_with("無")
        {
            return false;
        }
        upper.starts_with("YES")
            || trimmed.starts_with('是')
            || trimmed.starts_with("有人")
    }
}

fn asset_item(path: &Path, description: &str) -> HashMap<String, String> {
    let mut item = HashMap::new();
    item.insert("path".into(), path.to_string_lossy().to_string());
    item.insert("description".into(), description.to_string());
    item
}

fn theme_excerpt(script_or_story: &str) -> String {
    let compact: String = script_or_story
        .split_whitespace()
        .take(40)
        .collect::<Vec<_>>()
        .join(" ");
    compact.chars().take(140).collect()
}

/// Drop human/crowd cues from LLM descriptions so image prompts stay empty-set.
fn strip_people_mentions(text: &str) -> String {
    let mut s = text.to_string();
    for p in [
        "人影", "人群", "人们", "行人", "顾客", "客人", "路人", "男人", "女人", "小孩", "儿童",
        "店员", "服务员", "职员", "乘客", "观众", "游客", "士兵", "警察", "司机", "老板", "主角",
        "角色", "身影", "背影", "侧影", "有人", "众人", "男女老少", "熙熙攘攘", "站着", "坐着的人",
    ] {
        s = s.replace(p, "");
    }
    if let Ok(re) = regex::RegexBuilder::new(
        r"(?i)\b(crowds?|people|persons?|someone|somebody|pedestrians?|passers?-?by|patrons?|customers?|waiters?|waitresses?|baristas?|tourists?|staff|silhouettes?|figures?|(?:a|the|several|many|few)\s+(?:man|woman|men|women|boy|girl|boys|girls|child|children|kids?))\b",
    )
    .build()
    {
        s = re.replace_all(&s, " ").into_owned();
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn safe_component(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Flatten registry into (path, description) pairs for frame reference selection.
pub fn world_asset_pairs(registry: &WorldAssetRegistry) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for group in ["environments", "props"] {
        if let Some(map) = registry.get(group) {
            for item in map.values() {
                if let (Some(p), Some(d)) = (item.get("path"), item.get("description")) {
                    out.push((PathBuf::from(p), d.clone()));
                }
            }
        }
    }
    out
}

/// Rank env/prop plates by overlap with the frame description (avoid wrong location).
pub fn rank_world_pairs_for_frame(
    frame_desc: &str,
    pairs: &[(PathBuf, String)],
    max: usize,
) -> Vec<(PathBuf, String)> {
    if pairs.is_empty() || max == 0 {
        return Vec::new();
    }
    let desc = frame_desc.to_ascii_lowercase();
    let mut scored: Vec<(i32, usize)> = pairs
        .iter()
        .enumerate()
        .map(|(i, (path, text))| {
            let blob = format!("{} {}", path.to_string_lossy(), text).to_ascii_lowercase();
            let mut score = 0i32;
            for tok in match_tokens(&blob) {
                if tok.chars().count() < 2 {
                    continue;
                }
                if desc.contains(&tok) {
                    score += if tok.chars().count() >= 4 { 3 } else { 1 };
                }
            }
            // Slight preference for environment plates over props when tied later.
            if blob.contains("environments") || blob.contains("empty environment") {
                score += 1;
            }
            (score, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    let mut out = Vec::new();
    for (score, i) in scored {
        if out.len() >= max {
            break;
        }
        // Keep weak matches only for the first env fallback.
        if score <= 1 && !out.is_empty() {
            continue;
        }
        out.push(pairs[i].clone());
    }
    if out.is_empty() {
        // Fallback: first environment plate if any, else first prop.
        if let Some(env) = pairs.iter().find(|(p, _)| {
            p.to_string_lossy()
                .to_ascii_lowercase()
                .contains("environments")
        }) {
            out.push(env.clone());
        } else {
            out.push(pairs[0].clone());
        }
    }
    out
}

fn match_tokens(blob: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    for ch in blob.chars() {
        if ch.is_ascii_alphanumeric() || (ch as u32) > 127 {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            toks.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    // Also add 2-char CJK bigrams from longer CJK runs for better Chinese matching.
    let cjk: String = blob
        .chars()
        .filter(|c| {
            let u = *c as u32;
            (0x4E00..=0x9FFF).contains(&u)
        })
        .collect();
    if cjk.chars().count() >= 2 {
        let chars: Vec<char> = cjk.chars().collect();
        for w in chars.windows(2) {
            toks.push(format!("{}{}", w[0], w[1]));
        }
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::{rank_world_pairs_for_frame, strip_people_mentions};
    use std::path::PathBuf;

    #[test]
    fn strips_people_keeps_set_words() {
        let out = strip_people_mentions(
            "A crowded coffee shop with people and permanent wood tables, a woman at the counter",
        );
        let lower = out.to_ascii_lowercase();
        assert!(lower.contains("coffee") || lower.contains("wood") || lower.contains("tables"));
        assert!(!lower.contains("people"));
        assert!(!lower.contains("woman"));
        assert!(lower.contains("permanent"));
    }

    #[test]
    fn ranks_matching_environment_higher() {
        let pairs = vec![
            (
                PathBuf::from("environments/0_INT_OFFICE/plate.png"),
                "GLOBAL EMPTY environment plate (no people): INT. OFFICE - DAY.".into(),
            ),
            (
                PathBuf::from("environments/1_INT_COFFEE_SHOP/plate.png"),
                "GLOBAL EMPTY environment plate (no people): INT. COFFEE SHOP - NIGHT.".into(),
            ),
            (
                PathBuf::from("props/0_mug/prop.png"),
                "GLOBAL prop bible (object only, no people): <mug>.".into(),
            ),
        ];
        let ranked = rank_world_pairs_for_frame(
            "Wide shot inside the coffee shop at night, steam rising",
            &pairs,
            2,
        );
        assert!(
            ranked[0]
                .0
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("coffee")
        );
    }
}
