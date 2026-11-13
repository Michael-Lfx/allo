//! Cloud email-OTP auth for Flowy.
//!
//! UI binds view-model state only. HTTP and token persistence live here;
//! callers never talk to Flowy envelopes directly.

mod client;
mod config;
mod error;
mod session;

pub use client::{
    AuthClient, UserProfile, display_name_from_email, normalize_email, normalize_otp,
    valid_email,
};
pub use config::{
    Config, DEFAULT_APP, DEFAULT_BASE_URL, DEFAULT_CHANNEL, EMAIL_OTP_COOLDOWN_SECS,
    EMAIL_OTP_LENGTH,
};
pub use error::AuthError;
pub use session::{Session, SessionStore};
