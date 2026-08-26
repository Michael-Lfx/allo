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
    use std::mem;

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    /// Return all currently running executable base-names (lower-cased).
    pub fn running_process_names() -> Vec<String> {
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(mem::size_of::<PROCESSENTRY32W>())
                .expect("PROCESSENTRY32W size fits in u32"),
            ..PROCESSENTRY32W::default()
        };
        let mut names = Vec::new();

        if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&unit| unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                if end > 0 {
                    names.push(String::from_utf16_lossy(&entry.szExeFile[..end]).to_lowercase());
                }
                if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };
        names
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

    #[cfg(target_os = "windows")]
    #[test]
    fn native_process_scan_finds_current_process() {
        let current = std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        assert!(platform::running_process_names().contains(&current));
    }
}
