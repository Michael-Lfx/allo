//! Shared look tokens for Flowy screens.

mod button;
mod chrome;
mod context;
mod controls;
mod feedback;
mod overlays;
mod theme;
mod tooltip;
mod typeface;

pub mod icons;

pub use button::{
    Button, IconButton, button, install_initial_tab_focus, interactive,
};
pub use chrome::{
    FilterTabs, ToolbarSearch, filter_chip, icon_segment, icon_segment_group,
};
pub use context::*;
pub use controls::*;
pub use feedback::*;
pub use overlays::*;
pub use theme::*;
pub use tooltip::tooltip_trigger;
pub use typeface::{
    PAGE_MAX_WIDTH, PageColumn, PageHeader, SectionHeader, Text, TextRole,
    Title, TitleLevel, TypeMetrics, page_inset, ui_font,
};

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
