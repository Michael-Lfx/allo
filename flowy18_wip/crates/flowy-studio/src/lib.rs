//! Studio workspace Entity: home / chat / automation.

mod automation;
mod chat;
mod composer;
mod credits;
mod feed;
pub mod library;
mod panel;
mod studio;

pub use composer::SubmitComposer;
pub use panel::{StudioConfig, StudioEvent, StudioPanel, StudioSurface};

pub use credits::{CreditsViewState, grouped as grouped_credits};
