//! Local tier directory: the temporary bridge between the four product tiers
//! and concrete cloud model ids.
//!
//! The v1 catalog has no tier metadata of its own, so the mapping lives here
//! and is intentionally isolated behind [`resolve_tier`]. When the v3 catalog
//! ships tier definitions, only this module (and its caller in `flowy-app`)
//! needs to change; the UI keeps consuming tier names and availability, never
//! raw model ids.

use crate::{CatalogModel, Model};

/// One cloud tier row and its local model binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierSpec {
    pub tier: Model,
    /// Rendered as a `Pro` badge; also implies a plan gate.
    pub pro_badge: bool,
    /// Minimum account level (0 = free, 1 = pro, 2 = ultra) before the v3
    /// catalog supplies its own `minTierLevel`.
    pub required_level: u8,
    /// Local mapping target. `None` means the tier is not backed by a cloud
    /// model (Flex, which is deferred).
    pub cloud_model_id: Option<&'static str>,
}

/// Cloud tiers in menu order (strongest first, matching the product menu).
pub const CLOUD_TIERS: [TierSpec; 3] = [
    TierSpec {
        tier: Model::Max,
        pro_badge: true,
        required_level: 1,
        cloud_model_id: Some("AIPC-deepseek-v4.1-flash"),
    },
    TierSpec {
        tier: Model::Standard,
        pro_badge: true,
        required_level: 1,
        // Temporary mapping: GLM 5.3 Flash is not in the v1 cloud catalog yet.
        cloud_model_id: Some("AIPC-deepseek-v4.1-flash"),
    },
    TierSpec {
        tier: Model::Lite,
        pro_badge: false,
        required_level: 0,
        cloud_model_id: Some("AIPC-qwen3.8-flash"),
    },
];

/// Result of looking one tier up in the current cloud catalog.
#[derive(Clone, Copy, Debug)]
pub enum TierResolution<'a> {
    Available(&'a CatalogModel),
    Unavailable,
}

#[must_use]
pub fn spec(tier: Model) -> Option<&'static TierSpec> {
    CLOUD_TIERS.iter().find(|spec| spec.tier == tier)
}

/// Resolves the selected tier against the current catalog. The match is
/// exact: a missing model must surface as unavailable instead of silently
/// selecting another catalog entry.
#[must_use]
pub fn resolve_tier(
    tier: Model,
    models: &[CatalogModel],
) -> TierResolution<'_> {
    let Some(spec) = spec(tier) else {
        return TierResolution::Unavailable;
    };
    let Some(target) = spec.cloud_model_id else {
        return TierResolution::Unavailable;
    };
    models
        .iter()
        .find(|model| model.id == target)
        .map_or(TierResolution::Unavailable, TierResolution::Available)
}

/// The stricter of the local tier requirement and the catalog-declared gate.
#[must_use]
pub fn required_level(tier: Model, model: Option<&CatalogModel>) -> u8 {
    let local = spec(tier).map_or(0, |spec| spec.required_level);
    let catalog = model.map_or(0, |model| model.min_tier_level);
    local.max(catalog)
}

/// `true` when the account's effective tier cannot select this tier.
#[must_use]
pub fn is_locked(
    tier: Model,
    effective_tier: u8,
    model: Option<&CatalogModel>,
) -> bool {
    required_level(tier, model) > effective_tier
}

#[cfg(test)]
mod tests {
    use super::{
        CLOUD_TIERS, TierResolution, is_locked, required_level, resolve_tier,
    };
    use crate::{CatalogModel, Model};

    fn model(id: &str, min_tier_level: u8) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            name: id.into(),
            endpoint: "https://example.invalid/claw/v1".into(),
            min_tier_level,
            ..CatalogModel::default()
        }
    }

    fn catalog() -> Vec<CatalogModel> {
        vec![
            model("AIPC-deepseek-v4.1-flash", 0),
            model("AIPC-qwen3.8-flash", 0),
        ]
    }

    #[test]
    fn menu_has_three_cloud_tiers_with_pro_badges() {
        assert_eq!(CLOUD_TIERS.len(), 3);
        assert_eq!(CLOUD_TIERS[0].tier, Model::Max);
        assert_eq!(CLOUD_TIERS[1].tier, Model::Standard);
        assert_eq!(CLOUD_TIERS[2].tier, Model::Lite);
        assert!(CLOUD_TIERS[0].pro_badge);
        assert!(CLOUD_TIERS[1].pro_badge);
        assert!(!CLOUD_TIERS[2].pro_badge);
        assert_eq!(CLOUD_TIERS[2].cloud_model_id, Some("AIPC-qwen3.8-flash"));
    }

    #[test]
    fn resolve_is_exact_and_missing_models_are_unavailable() {
        let models = catalog();
        let TierResolution::Available(found) =
            resolve_tier(Model::Lite, &models)
        else {
            panic!("lite should resolve");
        };
        assert_eq!(found.id, "AIPC-qwen3.8-flash");
        assert!(matches!(
            resolve_tier(Model::Max, &[model("AIPC-other", 0)]),
            TierResolution::Unavailable
        ));
        assert!(matches!(
            resolve_tier(Model::Flex, &models),
            TierResolution::Unavailable
        ));
    }

    #[test]
    fn lock_combines_local_requirement_and_catalog_gate() {
        let open = model("AIPC-qwen3.8-flash", 0);
        let gated = model("AIPC-deepseek-v4.1-flash", 2);
        assert!(!is_locked(Model::Lite, 0, Some(&open)));
        assert!(is_locked(Model::Max, 0, Some(&open)));
        assert!(!is_locked(Model::Max, 1, Some(&open)));
        assert_eq!(required_level(Model::Max, Some(&gated)), 2);
        assert!(is_locked(Model::Max, 1, Some(&gated)));
        assert!(!is_locked(Model::Max, 2, Some(&gated)));
    }
}
