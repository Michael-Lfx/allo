//! Shared look tokens for Flowy screens.

mod theme;

pub use theme::*;

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Accent {
    #[default]
    Default = 0,
    Blue = 1,
    Green = 2,
    Orange = 3,
    Red = 4,
    Purple = 5,
}

impl Accent {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Blue,
        Self::Green,
        Self::Orange,
        Self::Red,
        Self::Purple,
    ];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Default => "look.default",
            Self::Blue => "look.blue",
            Self::Green => "look.green",
            Self::Orange => "look.orange",
            Self::Red => "look.red",
            Self::Purple => "look.purple",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Density {
    #[default]
    Compact = 0,
    Regular = 1,
    Comfortable = 2,
}

impl Density {
    pub const ALL: [Self; 3] =
        [Self::Compact, Self::Regular, Self::Comfortable];

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Compact => "look.compact",
            Self::Regular => "look.regular",
            Self::Comfortable => "look.comfortable",
        }
    }
}
