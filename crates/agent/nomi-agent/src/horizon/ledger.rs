//! Mechanical progress ledger for long-horizon Goal / Plan loops.
//!
//! The judge sees prose. This ledger sees the world: mutations, verification
//! commands, workspace fingerprints, and near-duplicate assistant text.
//! Recon-only tool success is recorded but does **not** count as progress.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{Instant, UNIX_EPOCH};

use nomi_coding::progress::is_recon_tool;
use nomi_coding::verify::{is_side_effect_tool, looks_like_verification_command};

use crate::profiler::{self, HotPath};

/// Consecutive natural EndTurns without world progress before Horizon stops
/// auto-continue. The first idle EndTurn still gets one delta continuation;
/// the second stops. Caps Codex-style empty requeue storms at one wasted round.
pub const NO_PROGRESS_STOP_STREAK: usize = 2;

/// One tool invocation Horizon needs to classify.
#[derive(Debug, Clone)]
pub struct ToolObservation {
    pub name: String,
    pub command: Option<String>,
    pub success: bool,
}

/// Snapshot copied onto `GoalState` so `update_goal` can refuse unverified Complete.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub mutated: bool,
    pub verify_ok: bool,
    pub workspace_changed: bool,
    pub no_progress_streak: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ProgressLedger {
    mutated: bool,
    verify_ok: bool,
    recon_only_turns: usize,
    last_workspace_fp: Option<u64>,
    last_text_sig: Option<u64>,
    last_pending_steps: usize,
    no_progress_streak: usize,
    workspace_changed: bool,
    last_idle_reason: Option<String>,
}

impl ProgressLedger {
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            mutated: self.mutated,
            verify_ok: self.verify_ok,
            workspace_changed: self.workspace_changed,
            no_progress_streak: self.no_progress_streak,
        }
    }

    pub fn no_progress_streak(&self) -> usize {
        self.no_progress_streak
    }

    pub fn last_idle_reason(&self) -> Option<&str> {
        self.last_idle_reason.as_deref()
    }

    pub fn had_progress(&self) -> bool {
        self.mutated || self.verify_ok || self.workspace_changed
    }

    /// New user message: keep fingerprints (so a restatement is still visible)
    /// but drop the idle streak — a human turn is not an auto-continue.
    pub fn on_user_request(&mut self) {
        self.no_progress_streak = 0;
        self.last_idle_reason = None;
        self.recon_only_turns = 0;
        self.last_pending_steps = 0;
        // Request-scoped mutation/verify restart; workspace_changed stays
        // until the next fingerprint sample so Complete can still see it.
        self.mutated = false;
        self.verify_ok = false;
    }

    /// After GoalState has copied this EndTurn's snapshot, drop round-scoped
    /// mutation/verify so the next auto-continue EndTurn is judged independently.
    /// `workspace_changed` stays: Complete still needs that evidence this request.
    pub fn consume_turn_scoped(&mut self) {
        self.mutated = false;
        self.verify_ok = false;
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub fn observe_tools(&mut self, tools: &[ToolObservation]) {
        if tools.is_empty() {
            return;
        }
        let mut any_progress = false;
        let mut all_recon = true;
        for tool in tools {
            let recon = is_recon_tool(&tool.name, tool.command.as_deref());
            if !recon {
                all_recon = false;
            }
            if tool.success
                && is_side_effect_tool(&tool.name)
                && !recon
            {
                self.mutated = true;
                any_progress = true;
            }
            if tool.success && tool.command.as_deref().is_some_and(looks_like_verification_command)
            {
                self.verify_ok = true;
                any_progress = true;
            }
        }
        if all_recon {
            self.recon_only_turns = self.recon_only_turns.saturating_add(1);
        } else if any_progress {
            self.recon_only_turns = 0;
        }
    }

    /// Natural EndTurn (no tool calls, or after a text-only close).
    ///
    /// Async because the workspace fingerprint shells out to `git`: running that
    /// synchronously inside the engine's async turn would block a runtime worker
    /// for as long as the subprocess takes.
    pub async fn observe_end_turn(
        &mut self,
        assistant_text: &str,
        cwd: Option<&Path>,
        pending_steps: usize,
    ) -> bool {
        let fp = workspace_fingerprint(cwd).await;
        let fp_changed = match (self.last_workspace_fp, fp) {
            (Some(prev), Some(now)) => prev != now,
            (None, Some(_)) => false,
            _ => false,
        };
        if let Some(now) = fp {
            self.last_workspace_fp = Some(now);
        }
        if fp_changed {
            self.workspace_changed = true;
        }

        let steps_changed = pending_steps > self.last_pending_steps;
        self.last_pending_steps = pending_steps;

        let text_sig = text_signature(assistant_text);
        let text_dup = self.last_text_sig == Some(text_sig) && text_sig != 0;
        if text_sig != 0 {
            self.last_text_sig = Some(text_sig);
        }

        let progress =
            self.mutated || self.verify_ok || fp_changed || steps_changed;
        if progress {
            self.no_progress_streak = 0;
            self.last_idle_reason = None;
            true
        } else {
            self.no_progress_streak = self.no_progress_streak.saturating_add(1);
            self.last_idle_reason = Some(idle_reason(text_dup, assistant_text.is_empty()));
            false
        }
    }

    pub fn idle_brief(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "- mutation this request: {}",
            if self.mutated { "yes" } else { "no" }
        ));
        lines.push(format!(
            "- verification command succeeded: {}",
            if self.verify_ok { "yes" } else { "no" }
        ));
        lines.push(format!(
            "- workspace fingerprint changed: {}",
            if self.workspace_changed { "yes" } else { "no" }
        ));
        lines.push(format!(
            "- consecutive EndTurns without progress: {}",
            self.no_progress_streak
        ));
        if let Some(reason) = &self.last_idle_reason {
            lines.push(format!("- idle reason: {reason}"));
        }
        lines.join("\n")
    }
}

fn idle_reason(text_dup: bool, empty: bool) -> String {
    if empty {
        "empty assistant reply".into()
    } else if text_dup {
        "assistant reply nearly identical to the previous EndTurn".into()
    } else {
        "no mutation, verification, or workspace change".into()
    }
}

pub fn text_signature(text: &str) -> u64 {
    let normalized: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(4000)
        .collect();
    if normalized.is_empty() {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    hasher.finish()
}

/// Fingerprint the workspace so a verification-visible change can be detected.
///
/// Prefers `git`; falls back to a directory listing when `cwd` is not a
/// repository. `None` means "there is no workspace to compare", which callers
/// treat as "unchanged".
pub async fn workspace_fingerprint(cwd: Option<&Path>) -> Option<u64> {
    let cwd = cwd?;
    if !cwd.exists() {
        return None;
    }
    match git_fingerprint(cwd).await {
        Some(fingerprint) => Some(fingerprint),
        None => Some(profiler::timed(HotPath::WorkspaceFingerprint, || {
            dir_fingerprint(cwd)
        })),
    }
}

/// One `git status --porcelain=v2 --branch` probe.
///
/// The v2 format prints `# branch.oid <oid>` before the entries, so this single
/// spawn covers what `rev-parse HEAD` plus `git status --porcelain` used to pay
/// two processes for on every EndTurn — and it distinguishes staged/unstaged
/// kinds and reports the checked-out head, which the old pair did not.
///
/// Awaited rather than blocking: a slow or hanging `git` must not pin a runtime
/// worker. Elapsed time is reported either way, including failed probes.
async fn git_fingerprint(cwd: &Path) -> Option<u64> {
    if !cwd.join(".git").exists() {
        return None;
    }
    let started = Instant::now();
    let output = nomi_process_runtime::hidden_command("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .output()
        .await
        .ok()?;
    profiler::record(HotPath::WorkspaceFingerprint, started.elapsed());
    if !output.status.success() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    hasher.write(&output.stdout);
    Some(hasher.finish())
}

fn dir_fingerprint(cwd: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Ok(meta) = std::fs::metadata(cwd) {
        hash_meta(&mut hasher, &meta);
    }
    if let Ok(rd) = std::fs::read_dir(cwd) {
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).take(64).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            hasher.write(entry.file_name().to_string_lossy().as_bytes());
            if let Ok(meta) = entry.metadata() {
                hash_meta(&mut hasher, &meta);
            }
        }
    }
    hasher.finish()
}

fn hash_meta(hasher: &mut DefaultHasher, meta: &std::fs::Metadata) {
    hasher.write_u64(meta.len());
    if let Ok(modified) = meta.modified()
        && let Ok(dur) = modified.duration_since(UNIX_EPOCH)
    {
        hasher.write_u64(dur.as_secs());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_ok() -> ToolObservation {
        ToolObservation {
            name: "Edit".into(),
            command: None,
            success: true,
        }
    }

    fn read_ok() -> ToolObservation {
        ToolObservation {
            name: "Read".into(),
            command: None,
            success: true,
        }
    }

    #[tokio::test]
    async fn recon_only_does_not_count_as_progress() {
        let mut ledger = ProgressLedger::default();
        ledger.observe_tools(&[read_ok()]);
        let progressed = ledger
            .observe_end_turn("I will look around more.", None, 0)
            .await;
        assert!(!progressed);
        assert_eq!(ledger.no_progress_streak(), 1);
        assert!(!ledger.snapshot().mutated);
    }

    #[tokio::test]
    async fn consume_turn_scoped_lets_later_endturn_count_as_idle() {
        let mut ledger = ProgressLedger::default();
        ledger.observe_tools(&[edit_ok()]);
        assert!(ledger.observe_end_turn("edited", None, 0).await);
        ledger.consume_turn_scoped();
        assert!(!ledger.snapshot().mutated);
        assert!(!ledger.observe_end_turn("just talking", None, 0).await);
        assert_eq!(ledger.no_progress_streak(), 1);
    }

    #[tokio::test]
    async fn successful_edit_resets_idle_streak() {
        let mut ledger = ProgressLedger::default();
        ledger.observe_end_turn("planning", None, 0).await;
        assert_eq!(ledger.no_progress_streak(), 1);
        ledger.observe_tools(&[edit_ok()]);
        let progressed = ledger.observe_end_turn("edited the file", None, 0).await;
        assert!(progressed);
        assert_eq!(ledger.no_progress_streak(), 0);
        assert!(ledger.snapshot().mutated);
    }

    #[tokio::test]
    async fn duplicate_endturn_text_is_idle() {
        let mut ledger = ProgressLedger::default();
        let text = "I will continue working on the feature now.";
        ledger.observe_end_turn(text, None, 0).await;
        ledger.observe_end_turn(text, None, 0).await;
        assert_eq!(ledger.no_progress_streak(), 2);
        assert!(ledger
            .last_idle_reason()
            .unwrap_or("")
            .contains("identical"));
    }

    #[tokio::test]
    async fn verify_command_counts_as_progress() {
        let mut ledger = ProgressLedger::default();
        ledger.observe_tools(&[ToolObservation {
            name: "Bash".into(),
            command: Some("cargo test -p nomi-agent".into()),
            success: true,
        }]);
        assert!(ledger.observe_end_turn("tests passed", None, 0).await);
        assert!(ledger.snapshot().verify_ok);
    }

    #[tokio::test]
    async fn pending_step_increase_counts_as_progress() {
        let mut ledger = ProgressLedger::default();
        assert!(!ledger.observe_end_turn("start", None, 0).await);
        assert!(ledger.observe_end_turn("planned two steps", None, 2).await);
        assert_eq!(ledger.no_progress_streak(), 0);
    }

    #[tokio::test]
    async fn user_request_clears_streak_but_keeps_fingerprint() {
        let mut ledger = ProgressLedger::default();
        ledger.observe_end_turn("idle", None, 0).await;
        ledger.on_user_request();
        assert_eq!(ledger.no_progress_streak(), 0);
        assert!(!ledger.snapshot().mutated);
    }

    #[tokio::test]
    async fn fingerprint_of_a_missing_directory_is_none() {
        assert_eq!(workspace_fingerprint(None).await, None);
        assert_eq!(
            workspace_fingerprint(Some(Path::new("does/not/exist"))).await,
            None
        );
    }

    #[tokio::test]
    async fn fingerprint_of_a_plain_directory_is_stable_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let before = workspace_fingerprint(Some(dir.path())).await;
        assert!(before.is_some(), "a readable directory must fingerprint");
        assert_eq!(
            before,
            workspace_fingerprint(Some(dir.path())).await,
            "an unchanged directory must fingerprint identically"
        );

        std::fs::write(dir.path().join("note.txt"), "x").unwrap();

        assert_ne!(
            before,
            workspace_fingerprint(Some(dir.path())).await,
            "a new file must move the fingerprint"
        );
    }

    /// The `git` probe is the call the engine used to make synchronously, twice
    /// per EndTurn. It must still be a real fingerprint, and it must be reported.
    #[tokio::test]
    async fn git_workspace_probe_tracks_a_new_file_and_reports_itself() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            return; // git is not installed here; the directory fallback is covered above.
        }
        let dir = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .output();
        if init.map(|output| !output.status.success()).unwrap_or(true) {
            return; // no usable git in this environment
        }

        let before = profiler_count(HotPath::WorkspaceFingerprint);

        let initial = workspace_fingerprint(Some(dir.path())).await;
        assert!(initial.is_some(), "a git worktree must fingerprint");
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let dirty = workspace_fingerprint(Some(dir.path())).await;
        assert_ne!(
            initial, dirty,
            "an untracked new file must move the git fingerprint"
        );

        assert!(
            profiler_count(HotPath::WorkspaceFingerprint) > before,
            "every probe must be reported to the profiler"
        );
    }

    fn profiler_count(path: crate::profiler::HotPath) -> u64 {
        crate::profiler::snapshot()
            .into_iter()
            .find(|sample| sample.path == path.as_str())
            .map(|sample| sample.count)
            .unwrap_or(0)
    }
}
