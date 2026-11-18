use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    #[default]
    Free,
    /// New name for the old `paid` plan; the alias keeps existing
    /// preferences readable while the enum gains a third tier.
    #[serde(alias = "paid")]
    Pro,
    Ultra,
}

impl Plan {
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Pro => 1,
            Self::Ultra => 2,
        }
    }

    /// Maps the cloud's `effectiveModelTier` (0 = free, 1 = pro, 2+ = ultra)
    /// onto the display plan. Authorization uses the raw tier, not this label.
    #[must_use]
    pub const fn from_effective_tier(tier: u8) -> Self {
        match tier {
            0 => Self::Free,
            1 => Self::Pro,
            _ => Self::Ultra,
        }
    }

    /// Display label key for the plan badge. Kept next to the enum so every
    /// surface (studio, settings, dock) renders the same three-state copy.
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Free => "credits.plan_free",
            Self::Pro => "credits.plan_pro",
            Self::Ultra => "credits.plan_ultra",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Max,
    Standard,
    #[default]
    Lite,
    Flex,
}

impl Model {
    pub const ALL: [Self; 4] =
        [Self::Max, Self::Standard, Self::Lite, Self::Flex];

    #[must_use]
    pub const fn name_key(self) -> &'static str {
        match self {
            Self::Max => "model.max",
            Self::Standard => "model.standard",
            Self::Lite => "model.lite",
            Self::Flex => "model.flex",
        }
    }

    #[must_use]
    pub const fn detail_key(self) -> &'static str {
        match self {
            Self::Max => "model.max_detail",
            Self::Standard => "model.standard_detail",
            Self::Lite => "model.lite_detail",
            Self::Flex => "model.flex_detail",
        }
    }

    #[must_use]
    pub const fn is_pro(self) -> bool {
        matches!(self, Self::Max | Self::Standard)
    }

    /// Plan level required to select this tier. Pro tiers require a paid
    /// account; Lite is open to every signed-in user.
    #[must_use]
    pub const fn required_level(self) -> u8 {
        match self {
            Self::Lite | Self::Flex => 0,
            Self::Standard | Self::Max => 1,
        }
    }

    /// Cloud tiers resolve to a catalog model; Flex is user-supplied and does
    /// not participate in the cloud directory.
    #[must_use]
    pub const fn is_cloud(self) -> bool {
        matches!(self, Self::Lite | Self::Standard | Self::Max)
    }

    #[must_use]
    pub const fn can_use(self, plan: Plan) -> bool {
        self.is_cloud() && self.required_level() <= plan.level()
    }
}

#[cfg(test)]
mod tests {
    use super::{Model, Plan};

    #[test]
    fn plan_levels_and_tier_requirements_are_monotonic() {
        assert_eq!(Plan::Free.level(), 0);
        assert_eq!(Plan::Pro.level(), 1);
        assert_eq!(Plan::Ultra.level(), 2);
        assert_eq!(Plan::from_effective_tier(0), Plan::Free);
        assert_eq!(Plan::from_effective_tier(1), Plan::Pro);
        assert_eq!(Plan::from_effective_tier(9), Plan::Ultra);

        assert_eq!(Model::Lite.required_level(), 0);
        assert_eq!(Model::Standard.required_level(), 1);
        assert_eq!(Model::Max.required_level(), 1);
        assert!(!Model::Flex.is_cloud());

        assert!(Model::Lite.can_use(Plan::Free));
        assert!(!Model::Standard.can_use(Plan::Free));
        assert!(Model::Standard.can_use(Plan::Pro));
        assert!(Model::Max.can_use(Plan::Ultra));
        assert!(!Model::Flex.can_use(Plan::Ultra));
    }
}
