use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    #[default]
    Free,
    Paid,
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

    #[must_use]
    pub const fn can_use(self, plan: Plan) -> bool {
        match self {
            Self::Lite => true,
            Self::Max | Self::Standard => matches!(plan, Plan::Paid),
            Self::Flex => false,
        }
    }
}
