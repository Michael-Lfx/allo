//! Goal wait-barrier liveness probe and background-process context, backed
//! by the agent process registry (`manager::process_registry`).
//!
//! The registry is the host's durable record of running agent subprocesses:
//! an entry exists while the process runs and is removed once its exit is
//! proven. That makes it the natural authority for the goal's pid/session
//! barriers without nomi-agent (or this crate) scanning the OS process
//! table. Both probe checks are deliberately fail-open — any read error or
//! missing entry answers "not alive", so a stale barrier releases at the
//! next evaluation point instead of wedging the goal loop.

use std::path::{Path, PathBuf};

use nomi_agent::goal::judge::BackgroundProcessInfo;
use nomi_agent::goal::runtime::GoalWaitProbe;

use crate::manager::process_registry::list_registered_processes;

/// [`GoalWaitProbe`] over the agent process registry.
///
/// Mapping (the registry has no separate process-runtime session notion):
/// - `is_pid_alive(pid)` — a process with that pid is still registered.
/// - `is_session_active(id)` — a process registered under that
///   `conversation_id` is still running. The background snapshot below sets
///   `BackgroundProcessInfo::session_id` to the same `conversation_id`, so a
///   judge-issued `wait_on_session` barrier round-trips through this check.
pub struct RegistryGoalWaitProbe {
    data_dir: PathBuf,
}

impl RegistryGoalWaitProbe {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }
}

impl GoalWaitProbe for RegistryGoalWaitProbe {
    fn is_pid_alive(&self, pid: u32) -> bool {
        list_registered_processes(&self.data_dir)
            .iter()
            .any(|p| p.pid == pid)
    }

    fn is_session_active(&self, session_id: &str) -> bool {
        list_registered_processes(&self.data_dir)
            .iter()
            .any(|p| p.conversation_id == session_id)
    }
}

/// Registry snapshot → judge background-process context for one
/// conversation. Fields the registry does not track (output preview, watch
/// patterns) stay at their inert defaults; `exited` is always `false`
/// because exited processes are unregistered.
pub fn conversation_background_processes(
    data_dir: &Path,
    conversation_id: &str,
) -> Vec<BackgroundProcessInfo> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    list_registered_processes(data_dir)
        .into_iter()
        .filter(|p| p.conversation_id == conversation_id)
        .map(|p| BackgroundProcessInfo {
            pid: p.pid,
            session_id: Some(p.conversation_id),
            command: p
                .command_preview
                .unwrap_or_else(|| format!("{} agent process", p.agent_type)),
            uptime_seconds: Some(now_ms.saturating_sub(p.registered_at_ms) / 1000),
            output_preview: None,
            watch_patterns: Vec::new(),
            watch_hit: false,
            notify_on_complete: false,
            exited: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a registry file in the exact on-disk shape
    /// `process_registry::write_registry_file` produces.
    fn write_registry(dir: &Path, entries_json: &str) {
        let path = dir.join(nomifun_common::dataset_roots::AGENT_PROCESS_REGISTRY_FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!(r#"{{"version":1,"processes":[{entries_json}]}}"#),
        )
        .unwrap();
    }

    #[test]
    fn probe_answers_from_registry_and_fails_open() {
        let dir = tempfile::tempdir().unwrap();
        let probe = RegistryGoalWaitProbe::new(dir.path().to_path_buf());

        // No registry file at all: fail open (nothing is alive).
        assert!(!probe.is_pid_alive(42));
        assert!(!probe.is_session_active("conv-1"));

        write_registry(
            dir.path(),
            r#"{"pid":42,"conversation_id":"conv-1","agent_type":"acp","registered_at_ms":123}"#,
        );
        assert!(probe.is_pid_alive(42));
        assert!(!probe.is_pid_alive(43));
        assert!(probe.is_session_active("conv-1"));
        assert!(!probe.is_session_active("conv-2"));

        // A corrupt registry also fails open instead of erroring.
        std::fs::write(
            dir.path()
                .join(nomifun_common::dataset_roots::AGENT_PROCESS_REGISTRY_FILE),
            "not json",
        )
        .unwrap();
        assert!(!probe.is_pid_alive(42));
    }

    #[test]
    fn background_snapshot_is_scoped_to_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        write_registry(
            dir.path(),
            concat!(
                r#"{"pid":42,"conversation_id":"conv-1","agent_type":"acp","#,
                r#""command_preview":"codex-acp","registered_at_ms":0},"#,
                r#"{"pid":77,"conversation_id":"conv-other","agent_type":"acp","registered_at_ms":0}"#,
            ),
        );

        let processes = conversation_background_processes(dir.path(), "conv-1");
        assert_eq!(processes.len(), 1);
        let p = &processes[0];
        assert_eq!(p.pid, 42);
        // session_id must round-trip through RegistryGoalWaitProbe::is_session_active.
        assert_eq!(p.session_id.as_deref(), Some("conv-1"));
        assert_eq!(p.command, "codex-acp");
        assert!(!p.exited);
        assert!(p.uptime_seconds.is_some());

        assert!(conversation_background_processes(dir.path(), "conv-none").is_empty());
    }
}
