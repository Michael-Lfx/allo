use async_trait::async_trait;

/// Payload for a host-level system notification (OS toast / browser Notification).
#[derive(Debug, Clone)]
pub struct SystemNotification {
    /// Auto-generated requirement title.
    pub title: String,
    pub status: NotificationStatus,
    /// Optional completion note.
    pub body: Option<String>,
    pub tag: String,
    /// Stable requirement id (`requirement_id`).
    pub task_id: String,
    /// Optional deep link for click-to-open.
    pub click_target: Option<String>,
    /// Source label (e.g. `"requirement"`).
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationStatus {
    Done,
    Failed,
    Cancelled,
    NeedsReview,
}

impl NotificationStatus {
    pub fn from_db(status: &str) -> Option<Self> {
        match status {
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "needs_review" => Some(Self::NeedsReview),
            _ => None,
        }
    }

    /// Short Chinese label for the notification body.
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Done => "已完成",
            Self::Failed => "失败",
            Self::Cancelled => "已取消",
            Self::NeedsReview => "待复核",
        }
    }
}

impl SystemNotification {
    /// Body line: status label, optionally followed by the completion note.
    pub fn body_text(&self) -> String {
        match self.body.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(note) => format!("{} — {note}", self.status.label_zh()),
            None => self.status.label_zh().to_owned(),
        }
    }
}

/// Non-fatal delivery error; callers log and swallow.
#[derive(Debug)]
pub enum NotifyError {
    Platform(String),
    PermissionDenied,
    Throttled,
}

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Platform(msg) => write!(f, "platform notification failed: {msg}"),
            Self::PermissionDenied => write!(f, "notification permission denied"),
            Self::Throttled => write!(f, "notification throttled"),
        }
    }
}

impl std::error::Error for NotifyError {}

/// Host-level system notification sink. Implementations must be cheap; the
/// requirement service already detaches the call onto a spawned task.
#[async_trait]
pub trait SystemTaskNotifier: Send + Sync {
    async fn notify(&self, message: &SystemNotification) -> Result<(), NotifyError>;
}
