//! Convert Flowy SkillCatalog entries into engine SkillMetadata.

use nomifun_common::AppError;
use nomifun_extension::{SkillPaths, list_catalog_skills, load_catalog_skills};
use nomi_skills::dynamic_skill_selector::{
    CheapSkillSelector, SkillSelectionDocument, WeightedLexicalSkillSelector,
};
use nomi_skills::frontmatter::{parse_frontmatter, parse_skill_fields};
use nomi_skills::prompt::get_char_budget;
use nomi_skills::types::{InvocationPolicy, LoadedFrom, SkillMetadata, SkillSource};

use crate::types::{CatalogDiscoveryEntry, CatalogDiscoveryHint};

/// Load every authorized catalog skill (body included) for the Nomi Skill tool.
pub async fn load_host_catalog_skills(paths: &SkillPaths) -> Result<Vec<SkillMetadata>, AppError> {
    let catalog = list_catalog_skills(paths)
        .await
        .map_err(AppError::from)?;
    if catalog.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = catalog
        .iter()
        .map(|item| {
            nomifun_api_types::SkillId::new(item.source, item.source_key.as_deref(), &item.local_key)
                .as_str()
                .to_owned()
        })
        .collect();
    let loaded = load_catalog_skills(paths, &ids)
        .await
        .map_err(AppError::from)?;
    Ok(loaded.into_iter().map(skill_metadata_from_catalog).collect())
}

pub fn skill_metadata_from_catalog(
    loaded: nomifun_extension::LoadedCatalogSkill,
) -> SkillMetadata {
    let parsed = parse_frontmatter(&loaded.content);
    let source = match loaded.source.as_str() {
        "builtin" => SkillSource::Bundled,
        "project" => SkillSource::Project,
        "mcp" | "extension" => SkillSource::Mcp,
        "legacy" => SkillSource::Legacy,
        _ => SkillSource::User,
    };
    let skill_root = loaded
        .location
        .as_deref()
        .and_then(|path| std::path::Path::new(path).parent())
        .map(|path| path.to_string_lossy().into_owned());
    let mut meta = parse_skill_fields(
        &parsed.frontmatter,
        &parsed.content,
        &loaded.name,
        source,
        LoadedFrom::Skills,
        skill_root.as_deref(),
    );
    meta.catalog_id = Some(loaded.skill_id);
    if meta.description.is_empty() {
        meta.description = loaded.description;
    }
    meta
}

pub fn apply_invocation_overrides(skills: &mut [SkillMetadata], overrides: &[(String, bool)]) {
    for skill in skills.iter_mut() {
        let Some(id) = skill.catalog_id.as_deref() else {
            continue;
        };
        if let Some((_, allow)) = overrides.iter().find(|(skill_id, _)| skill_id == id) {
            skill.disable_model_invocation = !*allow;
            skill.invocation_policy = if *allow {
                InvocationPolicy::ModelAuto
            } else {
                InvocationPolicy::SlashOnly
            };
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogRankInput<'a> {
    pub skill_id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub paths: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogRankingMode {
    /// Persist ranked ids, keep the full L1 listing unchanged.
    Shadow,
    /// When the listing exceeds budget, inject the ranked subset into the turn.
    ApplyWhenTruncated,
}

impl CatalogRankingMode {
    pub fn from_env() -> Self {
        match std::env::var("NOMI_SKILL_RANKING_SHADOW_ONLY") {
            Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => Self::Shadow,
            _ => Self::ApplyWhenTruncated,
        }
    }
}

pub fn implied_attachment_globs(name: &str, description: &str) -> Vec<String> {
    let hay = format!("{name} {description}").to_ascii_lowercase();
    let mut globs = Vec::new();
    if hay.contains(".xlsx")
        || hay.contains(".csv")
        || hay.contains("spreadsheet")
        || hay.contains("excel")
    {
        globs.push("**/*.xlsx".to_owned());
        globs.push("**/*.csv".to_owned());
    }
    if hay.contains(".docx") || hay.contains("word doc") {
        globs.push("**/*.docx".to_owned());
    }
    if hay.contains(".pptx") || hay.contains("slide") || hay.contains("pitch") {
        globs.push("**/*.pptx".to_owned());
    }
    globs
}

pub fn rank_catalog_for_turn(
    query: &str,
    items: &[CatalogRankInput<'_>],
    attached_files: &[String],
    force_ids: &[String],
    context_window_tokens: Option<usize>,
) -> CatalogDiscoveryHint {
    rank_catalog_for_turn_with_mode(
        query,
        items,
        attached_files,
        force_ids,
        context_window_tokens,
        CatalogRankingMode::from_env(),
    )
}

pub fn rank_catalog_for_turn_with_mode(
    query: &str,
    items: &[CatalogRankInput<'_>],
    attached_files: &[String],
    force_ids: &[String],
    context_window_tokens: Option<usize>,
    mode: CatalogRankingMode,
) -> CatalogDiscoveryHint {
    let started = std::time::Instant::now();
    let listing_truncated = catalog_listing_exceeds_budget(items, context_window_tokens);
    let documents: Vec<SkillSelectionDocument<'_>> = items
        .iter()
        .map(|item| SkillSelectionDocument {
            id: item.skill_id,
            name: item.name,
            short_description: "",
            description: item.description,
            paths: item.paths,
        })
        .collect();
    let ranked = WeightedLexicalSkillSelector.select(query, &documents, 12, attached_files);
    let apply = listing_truncated
        && mode == CatalogRankingMode::ApplyWhenTruncated
        && !ranked.candidate_ids.is_empty();
    let entries = if apply {
        ranked
            .candidate_ids
            .iter()
            .filter_map(|id| items.iter().find(|item| item.skill_id == id))
            .filter(|item| !force_ids.iter().any(|forced| forced == item.skill_id))
            .map(|item| CatalogDiscoveryEntry {
                skill_id: item.skill_id.to_owned(),
                name: item.name.to_owned(),
                description: item.description.to_owned(),
            })
            .collect()
    } else {
        Vec::new()
    };
    CatalogDiscoveryHint {
        ranked_ids: ranked.candidate_ids,
        listing_truncated,
        method: ranked.method.to_owned(),
        entries,
        applied_to_l1: apply,
        latency_ms: started.elapsed().as_millis() as i64,
    }
}

fn catalog_listing_exceeds_budget(
    items: &[CatalogRankInput<'_>],
    context_window_tokens: Option<usize>,
) -> bool {
    if items.is_empty() {
        return false;
    }
    let budget = get_char_budget(context_window_tokens);
    let total: usize = items
        .iter()
        .map(|item| item.name.len() + item.skill_id.len() + item.description.len() + 8)
        .sum();
    total > budget || items.len() > 40
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_conversion_sets_canonical_id() {
        let meta = skill_metadata_from_catalog(nomifun_extension::LoadedCatalogSkill {
            skill_id: "builtin:officecli".to_owned(),
            name: "officecli".to_owned(),
            description: "Office documents".to_owned(),
            source: "builtin".to_owned(),
            version_hash: "a".repeat(64),
            content: "---\nname: officecli\ndescription: Office documents\n---\nUse officecli.\n"
                .to_owned(),
            location: Some("/tmp/officecli/SKILL.md".to_owned()),
        });
        assert_eq!(meta.catalog_id.as_deref(), Some("builtin:officecli"));
        assert_eq!(meta.name, "officecli");
        assert!(!meta.disable_model_invocation);
    }

    #[test]
    fn slash_only_frontmatter_hides_from_model() {
        let meta = skill_metadata_from_catalog(nomifun_extension::LoadedCatalogSkill {
            skill_id: "user:deploy".to_owned(),
            name: "deploy".to_owned(),
            description: "Deploy".to_owned(),
            source: "user".to_owned(),
            version_hash: "b".repeat(64),
            content: "---\nname: deploy\ndescription: Deploy production\ninvocation-policy: /invoke\n---\nDangerous.\n"
                .to_owned(),
            location: None,
        });
        assert!(meta.disable_model_invocation);
        assert!(meta.invocation_policy.is_slash_only());
    }

    fn rank_item<'a>(
        skill_id: &'a str,
        name: &'a str,
        description: &'a str,
        paths: &'a [String],
    ) -> CatalogRankInput<'a> {
        CatalogRankInput {
            skill_id,
            name,
            description,
            paths,
        }
    }

    #[test]
    fn ranking_stays_shadow_until_listing_is_truncated() {
        let items = vec![rank_item(
            "builtin:officecli",
            "officecli",
            "office documents",
            &[],
        )];
        let hint = rank_catalog_for_turn_with_mode(
            "officecli",
            &items,
            &[],
            &[],
            Some(200_000),
            CatalogRankingMode::ApplyWhenTruncated,
        );
        assert!(!hint.listing_truncated);
        assert!(!hint.applied_to_l1);
        assert!(hint.entries.is_empty());
        assert_eq!(hint.ranked_ids, vec!["builtin:officecli".to_owned()]);
    }

    #[test]
    fn truncated_listing_applies_topk_unless_shadow() {
        let owned: Vec<(String, String, String)> = (0..45)
            .map(|i| {
                (
                    format!("builtin:skill-{i:02}"),
                    format!("skill-{i:02}"),
                    format!("generic helper number {i}"),
                )
            })
            .collect();
        let items: Vec<CatalogRankInput<'_>> = owned
            .iter()
            .map(|(id, name, desc)| rank_item(id, name, desc, &[]))
            .collect();
        let applied = rank_catalog_for_turn_with_mode(
            "skill-41",
            &items,
            &[],
            &[],
            Some(1_000),
            CatalogRankingMode::ApplyWhenTruncated,
        );
        assert!(applied.listing_truncated);
        assert!(applied.applied_to_l1);
        assert_eq!(applied.ranked_ids[0], "builtin:skill-41");
        assert!(!applied.entries.is_empty());

        let shadow = rank_catalog_for_turn_with_mode(
            "skill-41",
            &items,
            &[],
            &[],
            Some(1_000),
            CatalogRankingMode::Shadow,
        );
        assert!(shadow.listing_truncated);
        assert!(!shadow.applied_to_l1);
        assert!(shadow.entries.is_empty());
        assert_eq!(shadow.ranked_ids, applied.ranked_ids);
    }

    #[test]
    fn attached_spreadsheet_boosts_xlsx_paths() {
        let xlsx_paths = vec!["**/*.xlsx".to_owned()];
        let items = vec![
            rank_item(
                "builtin:officecli-xlsx",
                "officecli-xlsx",
                "spreadsheets",
                &xlsx_paths,
            ),
            rank_item(
                "builtin:writer",
                "writer",
                "spreadsheets narrative",
                &[],
            ),
        ];
        let hint = rank_catalog_for_turn_with_mode(
            "spreadsheets",
            &items,
            &["C:/tmp/book.xlsx".to_owned()],
            &[],
            Some(1_000),
            CatalogRankingMode::Shadow,
        );
        assert_eq!(hint.ranked_ids[0], "builtin:officecli-xlsx");
    }

    #[test]
    fn implied_globs_cover_office_filenames() {
        let globs = implied_attachment_globs("officecli-xlsx", "create a .xlsx workbook");
        assert!(globs.iter().any(|glob| glob.ends_with(".xlsx")));
    }

    #[test]
    fn session_pilot_sample_skills_default_to_model_auto() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../nomifun-app/assets/builtin-skills");
        const PILOT: &[&str] = &[
            "officecli",
            "officecli-docx",
            "officecli-xlsx",
            "officecli-pptx",
            "news-briefing",
            "planning-with-files",
            "ui-ux-pro-max",
            "star-office-helper",
            "art-motion",
            "nomifun-webui-setup",
        ];
        for name in PILOT {
            let skill_md = std::fs::read_to_string(root.join(name).join("SKILL.md"))
                .unwrap_or_else(|error| panic!("missing pilot skill {name}: {error}"));
            let meta = skill_metadata_from_catalog(nomifun_extension::LoadedCatalogSkill {
                skill_id: format!("builtin:{name}"),
                name: (*name).to_owned(),
                description: String::new(),
                source: "builtin".to_owned(),
                version_hash: "c".repeat(64),
                content: skill_md,
                location: Some(root.join(name).join("SKILL.md").display().to_string()),
            });
            if *name == "nomifun-webui-setup" {
                assert!(
                    meta.disable_model_invocation,
                    "{name} is the slash-only pilot"
                );
            } else {
                assert!(
                    !meta.disable_model_invocation,
                    "{name} should stay model-auto"
                );
            }
            if *name == "art-motion" {
                assert!(
                    meta.requires_confirmation,
                    "script skill must be confirmation-gated"
                );
            }
        }
    }
}
