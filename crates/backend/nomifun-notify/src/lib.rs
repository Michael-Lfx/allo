//! OS / host system notifications for completed requirements.
//!
//! Dependency direction mirrors `nomifun-webhook`: this crate depends on
//! `nomifun-requirement` (for [`CompletionNotifier`]); requirement does not
//! depend on this crate. Desktop hosts attach a platform notifier through
//! [`SystemNotifierSlot`] after the Tauri `AppHandle` exists.

mod completion;
mod log_notifier;
mod slot;
mod types;

pub use completion::SystemCompletionNotifier;
pub use log_notifier::LogNotifier;
pub use slot::SystemNotifierSlot;
pub use types::{NotificationStatus, NotifyError, SystemNotification, SystemTaskNotifier};
