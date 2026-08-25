//! Process-based automatic meeting trigger.
//!
//! Polls the system process list every `poll_interval` and fires a callback
//! the first time a recognised conferencing application is detected.  A
//! second callback fires when all such processes have exited (meeting ended).
//!
//! # Recognised applications
//!
//! | App | Executable(s) |
//! |-----|--------------|
//! | Tencent Meeting (腾讯会议) | `WeMeetApp.exe`, `TencentMeeting.exe` |
//! | Feishu / Lark (飞书) | `Lark.exe`, `LarkMeetingAddin.exe` |
//! | DingTalk (钉钉) | `DingTalk.exe` |
//! | Microsoft Teams | `ms-teams.exe`, `Teams.exe` |
//! | Zoom | `Zoom.exe` |
//!
//! # Example
//!
//! ```rust,ignore
//! use nomifun_audio::process_watch::ProcessWatcher;
//! use std::time::Duration;
//!
//! let mut watcher = ProcessWatcher::new(Duration::from_secs(5));
//! watcher.on_started(|app| println!("Meeting started: {app}"));
//! watcher.on_ended(|app| println!("Meeting ended: {app}"));
//! watcher.run().await;
//! ```

use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, info};

/// Well-known meeting application names (lower-cased on comparison).
const MEETING_PROCESSES: &[(&str, &str)] = &[
    ("wemeetapp.exe", "Tencent Meeting"),
    ("tencentmeeting.exe", "Tencent Meeting"),
    ("lark.exe", "Feishu / Lark"),
    ("larkmeetingaddin.exe", "Feishu / Lark"),
    ("dingtalk.exe", "DingTalk"),
    ("ms-teams.exe", "Microsoft Teams"),
    ("teams.exe", "Microsoft Teams"),
    ("zoom.exe", "Zoom"),
];

type MeetingCallback = Option<Box<dyn Fn(&str) + Send + Sync>>;

/// Detects meeting application process start/stop and fires callbacks.
pub struct ProcessWatcher {
    poll_interval: Duration,
    on_started: MeetingCallback,
    on_ended: MeetingCallback,
}

impl ProcessWatcher {
    pub fn new(poll_interval: Duration) -> Self {
        Self {
            poll_interval,
            on_started: None,
            on_ended: None,
        }
    }

    /// Called with the app name the first time a meeting process appears.
    pub fn on_started(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_started = Some(Box::new(f));
        self
    }

    /// Called with the app name when the last matching process exits.
    pub fn on_ended(mut self, f: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_ended = Some(Box::new(f));
        self
    }

    /// Run the polling loop until the task is cancelled.
    pub async fn run(self) {
        let mut ticker = interval(self.poll_interval);
        let mut active_app: Option<String> = None;

        loop {
            ticker.tick().await;

            let detected = detect_meeting_process();
            debug!("ProcessWatcher: scan result = {detected:?}");

            match (&active_app, detected) {
                (None, Some(app)) => {
                    info!("ProcessWatcher: meeting started — {app}");
                    if let Some(ref cb) = self.on_started {
                        cb(&app);
                    }
                    active_app = Some(app);
                }
                (Some(prev), None) => {
                    info!("ProcessWatcher: meeting ended — {prev}");
                    if let Some(ref cb) = self.on_ended {
                        cb(prev);
                    }
                    active_app = None;
                }
                _ => {}
            }
        }
    }
}

/// Scan the running process list for known meeting apps.
///
/// Returns the human-readable name of the first match, or `None`.
pub fn detect_meeting_process() -> Option<String> {
    platform::running_process_names()
        .into_iter()
        .find_map(|name| {
            let lower = name.to_lowercase();
            MEETING_PROCESSES
                .iter()
                .find(|(exe, _)| lower == *exe)
                .map(|(_, label)| label.to_string())
        })
}

// ---------------------------------------------------------------------------
// Platform implementations
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod platform {
    /// Return all currently running executable base-names (lower-cased).
    pub fn running_process_names() -> Vec<String> {
        // Use the Windows `tasklist` command as a dependency-free fallback.
        // A proper implementation would use `CreateToolhelp32Snapshot` /
        // `Process32Next`, but that requires unsafe COM calls.  `tasklist`
        // is always present on Windows and is sufficient for a 5-second poll.
        let output = std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output()
            .unwrap_or_else(|_| std::process::Output {
                status: std::process::ExitStatus::default(),
                stdout: vec![],
                stderr: vec![],
            });

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                // CSV format: "ImageName","PID","Session","SessionNum","MemUsage"
                let name = line.trim_matches('"').split('"').next()?;
                Some(name.to_lowercase())
            })
            .collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub fn running_process_names() -> Vec<String> {
        // Stub: always returns empty on non-Windows.
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_no_meeting_in_test_env() {
        // In CI / test environments there should be no meeting app running.
        // We just ensure the function doesn't panic.
        let _ = detect_meeting_process();
    }
}
