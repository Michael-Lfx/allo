//! Shared look tokens for Flowy screens.
//!
//! Before restructuring a screen, read "GPUI layout traps" in the crate-level
//! `DESIGN.md`: `w_full` collapsing inside centred rows, `scrollbar_width(0)`
//! disabling scroll, stale `viewport_size`, and unreliable absolute positioning
//! are all behaviours that compile fine and fail only when run.

mod button;
mod carousel;
pub mod charts;
mod chrome;
mod context;
mod controls;

mod feedback;
pub mod motion;
mod overlays;
pub mod scroll;
mod theme;
mod tooltip;
mod typeface;

pub mod icons;

pub use button::{
    Button, IconButton, button, install_initial_tab_focus, interactive,
};
pub use carousel::{CarouselGeometry, CarouselState, carousel_track};
pub use charts::{
    DonutSegment, FlowEdgeStyle, donut_chart, flow_connector, progress_ring,
};
pub use chrome::{
    FilterTabs, ToolbarSearch, filter_chip, icon_segment, icon_segment_group,
};
pub use context::*;
pub use controls::*;
pub use feedback::*;
pub use overlays::*;
pub use scroll::{
    ScrollArea, ScrollbarAxis, ScrollbarMode, scroll_area, scroll_list_host,
};
pub use theme::*;
pub use tooltip::tooltip_trigger;
pub use typeface::{
    PAGE_MAX_WIDTH, PageColumn, PageHeader, SectionHeader, Text, TextRole,
    Title, TitleLevel, TypeMetrics, page_inset, ui_font,
};

use gpui::{Div, Stateful};
use serde::{Deserialize, Serialize};

/// The element type returned by the crate's interactive builders.
///
/// With the `test-support` feature this is an observation wrapper that
/// registers the node for UI queries; without it the builders return the plain
/// `Stateful<Div>` unchanged. Callers therefore see one type in both builds, and
/// release code keeps the exact element it always had.
///
/// Observation must stay at the *end* of a builder chain: `Observed` forwards
/// `Styled`, `ParentElement` and `InteractiveElement`, so later calls still
/// work, but its `PrepaintState` stops being `Option<Hitbox>` and a second
/// `test_support()` becomes an identity call.
pub type InteractiveDiv = gpui_kit::base::ObservedElement<Stateful<Div>>;

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
