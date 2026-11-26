//! Studio workspace Entity: home / chat / automation.

mod automation;
mod chat;
mod composer;
mod composer_add_menu;
mod credits;
mod deliverables;
mod feed;
mod file_kind;
mod history;
mod home;
pub mod library;
mod markdown;
mod menus;
mod motion;
mod overview;
mod panel;
mod pending_card;
pub mod ports;
mod projects;
mod starter;
mod studio;
mod work_actions;

pub use composer::SubmitComposer;
pub use panel::{
    StudioConfig, StudioEvent, StudioPanel, StudioSurface, TierView,
};
pub use ports::{SkillChip, WorkBoardSnapshot};
pub use projects::Project;

pub use credits::{CreditsViewState, grouped as grouped_credits};

/// Shared maximum stage width for the Home hero, composer, starters, and chat stream.
pub(crate) const STAGE_MAX_W: f32 = 760.;
