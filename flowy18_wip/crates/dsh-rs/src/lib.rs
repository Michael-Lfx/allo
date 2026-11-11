//! # dsh-rs
//!
//! The dsh agent harness as a single crate. **External consumers** should
//! depend only on [`api`], [`types`], and [`bundle`] (`install_*`).
//! Implementation modules stay crate-private unless the `unstable` feature is
//! enabled (CLI binary and integration tests).

pub mod api;
pub mod bundle;
pub mod types;

#[cfg(feature = "unstable")]
pub mod cli;
#[cfg(feature = "unstable")]
pub mod core;
#[cfg(feature = "unstable")]
pub mod llm;
#[cfg(feature = "unstable")]
pub mod session;
#[cfg(feature = "unstable")]
pub mod tools;

#[cfg(not(feature = "unstable"))]
pub(crate) mod cli;
#[cfg(not(feature = "unstable"))]
pub(crate) mod core;
#[cfg(not(feature = "unstable"))]
pub(crate) mod llm;
#[cfg(not(feature = "unstable"))]
pub(crate) mod session;
#[cfg(not(feature = "unstable"))]
pub(crate) mod tools;
