//! Document model, persistence, and editing sessions.
//!
//! Geometry and search keep older style; allow the lints that would force
//! noise rewrites without changing behavior.

#![allow(clippy::assert_is_empty)]
#![allow(clippy::many_single_char_names)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::needless_range_loop)]
#![allow(clippy::question_mark)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::suboptimal_flops)]

pub mod geometry;
pub mod history;
pub mod model;
pub mod persistence;
pub mod search;
pub mod session;

pub use session::DocumentSession;
