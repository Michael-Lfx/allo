//! Studio workspace Entity: home / chat / automation.

mod automation;
mod chat;
mod composer;
mod credits;
mod deliverables;
mod feed;
mod file_kind;
mod history;
mod motion;
pub mod library;
mod markdown;
mod panel;
mod projects;
mod starter;
mod studio;

pub use composer::SubmitComposer;
pub use panel::{
    StudioConfig, StudioEvent, StudioPanel, StudioSurface, TierView,
};
pub use projects::Project;

pub use credits::{CreditsViewState, grouped as grouped_credits};
