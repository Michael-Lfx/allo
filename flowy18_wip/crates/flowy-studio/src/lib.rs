//! Studio workspace Entity: home / chat / automation.

mod automation;
mod chat;
mod composer;
mod credits;
mod feed;
mod history;
pub mod library;
mod markdown;
mod panel;
mod projects;
mod studio;

pub use composer::SubmitComposer;
pub use panel::{
    StudioConfig, StudioEvent, StudioPanel, StudioSurface, TierView,
};
pub use projects::Project;

pub use credits::{CreditsViewState, grouped as grouped_credits};
