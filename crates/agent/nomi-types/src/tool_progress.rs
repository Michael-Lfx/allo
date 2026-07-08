//! Scoped tool execution progress for long-running media workflows.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

type Reporter = Arc<dyn Fn(&str) + Send + Sync>;

struct Slot {
    reporter: Option<Reporter>,
}

fn slot() -> MutexGuard<'static, Slot> {
    static SLOT: Mutex<Slot> = Mutex::new(Slot { reporter: None });
    SLOT.lock().unwrap_or_else(|e| e.into_inner())
}

fn detached_reporters() -> MutexGuard<'static, HashMap<String, Reporter>> {
    static DETACHED: LazyLock<Mutex<HashMap<String, Reporter>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    DETACHED.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct DetachedToolProgressGuard {
    run_id: String,
}

impl DetachedToolProgressGuard {
    pub fn attach(run_id: impl Into<String>) -> Option<Self> {
        let run_id = run_id.into();
        let reporter = slot().reporter.clone()?;
        detached_reporters().insert(run_id.clone(), reporter);
        Some(Self { run_id })
    }
}

impl Drop for DetachedToolProgressGuard {
    fn drop(&mut self) {
        detached_reporters().remove(&self.run_id);
    }
}

pub fn report_tool_progress(message: impl AsRef<str>) {
    let message = message.as_ref();
    if let Some(reporter) = slot().reporter.clone() {
        reporter(message);
        return;
    }
    let detached = detached_reporters();
    for reporter in detached.values() {
        reporter(message);
    }
}
