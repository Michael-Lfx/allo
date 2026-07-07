//! In-flight workflow run control (cancellation, video task tracking).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use tokio::task::AbortHandle;

struct ActiveRun {
    cancelled: Arc<AtomicBool>,
    abort_handle: AbortHandle,
    active_video_task: Arc<RwLock<Option<i64>>>,
}

/// Registry for running workflow tasks — supports cancel and server-side video abort.
#[derive(Clone, Default)]
pub struct WorkflowRunControl {
    inner: Arc<RwLock<HashMap<String, ActiveRun>>>,
}

impl WorkflowRunControl {
    pub fn register(&self, run_id: &str, abort_handle: AbortHandle) -> Arc<AtomicBool> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let entry = ActiveRun {
            cancelled: Arc::clone(&cancelled),
            abort_handle,
            active_video_task: Arc::new(RwLock::new(None)),
        };
        if let Ok(mut guard) = self.inner.write() {
            guard.insert(run_id.to_string(), entry);
        }
        cancelled
    }

    pub fn unregister(&self, run_id: &str) {
        if let Ok(mut guard) = self.inner.write() {
            guard.remove(run_id);
        }
    }

    pub fn is_cancelled(&self, run_id: &str) -> bool {
        self.inner
            .read()
            .ok()
            .and_then(|g| g.get(run_id).map(|r| r.cancelled.load(Ordering::Acquire)))
            .unwrap_or(false)
    }

    pub fn set_active_video_task(&self, run_id: &str, local_id: i64) {
        if let Ok(guard) = self.inner.read()
            && let Some(run) = guard.get(run_id)
            && let Ok(mut task) = run.active_video_task.write()
        {
            *task = Some(local_id);
        }
    }

    pub fn active_video_task(&self, run_id: &str) -> Option<i64> {
        self.inner.read().ok().and_then(|g| {
            g.get(run_id)?
                .active_video_task
                .read()
                .ok()
                .and_then(|t| *t)
        })
    }

    pub fn clear_active_video_task(&self, run_id: &str) {
        if let Ok(guard) = self.inner.read()
            && let Some(run) = guard.get(run_id)
            && let Ok(mut task) = run.active_video_task.write()
        {
            *task = None;
        }
    }

    /// Mark run cancelled, abort local task, return server video task id if any.
    pub fn cancel(&self, run_id: &str) -> Option<i64> {
        let (abort, video_id) = {
            let guard = self.inner.read().ok()?;
            let run = guard.get(run_id)?;
            run.cancelled.store(true, Ordering::Release);
            let video_id = run.active_video_task.read().ok().and_then(|t| *t);
            (run.abort_handle.clone(), video_id)
        };
        abort.abort();
        video_id
    }

    pub fn contains(&self, run_id: &str) -> bool {
        self.inner
            .read()
            .ok()
            .is_some_and(|g| g.contains_key(run_id))
    }
}
