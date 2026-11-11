//! Pre-tracing boot notes (packaged Windows has no console).

use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootNoteLevel {
    Info,
    Warn,
}

static BOOT_NOTES: Mutex<Vec<(BootNoteLevel, String)>> = Mutex::new(Vec::new());

pub fn record_boot_note(level: BootNoteLevel, message: impl Into<String>) {
    if let Ok(mut notes) = BOOT_NOTES.lock() {
        notes.push((level, message.into()));
    }
}

pub fn drain_boot_notes() -> Vec<(BootNoteLevel, String)> {
    BOOT_NOTES
        .lock()
        .map(|mut notes| std::mem::take(&mut *notes))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_then_drain_is_fifo() {
        // Isolate from other tests by draining first.
        let _ = drain_boot_notes();
        record_boot_note(BootNoteLevel::Info, "first");
        record_boot_note(BootNoteLevel::Warn, "second");
        assert_eq!(
            drain_boot_notes(),
            vec![
                (BootNoteLevel::Info, "first".into()),
                (BootNoteLevel::Warn, "second".into()),
            ]
        );
        assert_eq!(drain_boot_notes(), []);
    }
}
