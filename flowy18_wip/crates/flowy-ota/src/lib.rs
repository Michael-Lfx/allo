//! Desktop over-the-air updates.
//!
//! Callers use [`Updater`] for network/install work and
//! [`DownloadedUpdateCache`] to keep download separate from install.

#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::case_sensitive_file_extension_comparisons)]

mod cache;
mod channel;
mod error;
mod hosts;
mod install_gate;
#[cfg(target_os = "linux")]
mod linux_install;
mod macos_swap;
mod manifest;
mod mirrors;
mod package;
mod rollout;
mod updater;
mod verify;
#[cfg(target_os = "windows")]
mod windows_install;

pub use cache::{
    BeginUpdateDownload, DownloadProgressCounters, DownloadedUpdateCache,
    PackageStatus, TakeReadyError, UpdatePackageCache,
};
pub use channel::ChannelConfig;
pub use error::UpdateError;
pub use updater::{AvailableUpdate, DownloadProgress, ReadyPackage, Updater};
