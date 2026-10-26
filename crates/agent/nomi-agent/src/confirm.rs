use std::collections::HashSet;
use std::io::{self, BufRead, Write};

pub struct ToolConfirmer {
    auto_approve: bool,
    /// Delegated Agents have no approval channel back to the host. When set,
    /// every tool that is not auto-approved or allow-listed is denied without
    /// reading stdin, where EOF used to be read as an empty "yes".
    deny_unconfirmed: bool,
    /// Monotonic count of calls this confirmer refused. Delegated engines
    /// carry it into the invocation result so a denied write cannot be
    /// reported as a completed task.
    denied_count: usize,
    allow_list: HashSet<String>,
    #[cfg(test)]
    check_count: usize,
    /// Test-only evidence that the interactive stdin prompt was reached. A
    /// delegated engine must deny before this point.
    #[cfg(test)]
    prompt_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    Approved,
    Denied(DenialReason),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialReason {
    User,
    NoApprovalChannel,
}

impl DenialReason {
    /// Model-facing explanation. Keep this actionable: the model should stop
    /// retrying and surface the real remedy instead of calling it a sandbox.
    pub fn message(self) -> &'static str {
        match self {
            Self::User => "Tool execution denied by user",
            Self::NoApprovalChannel => {
                "Tool execution denied: no approval channel is available. Add this tool to [tools].allow_list, enable auto-approve (YOLO), or run the write in a session with an approval channel (for delegated tasks, the parent agent)."
            }
        }
    }
}

impl ToolConfirmer {
    pub fn new(auto_approve: bool, allow_list: Vec<String>) -> Self {
        Self {
            auto_approve,
            deny_unconfirmed: false,
            denied_count: 0,
            allow_list: allow_list.into_iter().collect(),
            #[cfg(test)]
            check_count: 0,
            #[cfg(test)]
            prompt_count: 0,
        }
    }

    /// Returns whether auto-approve is enabled
    pub fn is_auto_approve(&self) -> bool {
        self.auto_approve
    }

    /// Fail closed for delegated engines: never prompt on stdin.
    pub fn deny_unconfirmed(&mut self) {
        self.deny_unconfirmed = true;
    }

    /// Number of tool calls this confirmer refused.
    pub fn denied_count(&self) -> usize {
        self.denied_count
    }

    /// Add a tool name to the allow list at runtime.
    /// Used by skill context modifiers to grant auto-approval for specified tools.
    pub fn add_to_allow_list(&mut self, name: &str) {
        self.allow_list.insert(name.to_string());
    }

    /// Check if the tool needs confirmation. Returns the user's decision.
    pub fn check(&mut self, tool_name: &str, tool_input_display: &str) -> ConfirmResult {
        #[cfg(test)]
        {
            self.check_count += 1;
        }

        if self.auto_approve || self.allow_list.contains(tool_name) {
            return ConfirmResult::Approved;
        }
        if self.deny_unconfirmed {
            return self.deny(DenialReason::NoApprovalChannel);
        }

        self.prompt_for_decision(tool_name, tool_input_display)
    }

    /// Interactive fallback used only when a host approval channel exists.
    /// Delegated engines return before this method, so they never read stdin.
    fn prompt_for_decision(&mut self, tool_name: &str, tool_input_display: &str) -> ConfirmResult {
        #[cfg(test)]
        {
            self.prompt_count += 1;
        }

        eprint!(
            "\n[tool] {}({})\nAllow? [y]es / [n]o / [a]lways / [q]uit > ",
            tool_name, tool_input_display
        );
        io::stderr().flush().unwrap();

        let mut input = String::new();
        // `Ok(0)` is EOF: an unattended child or a closed stdin must not be
        // read as consent. Only an explicit y/yes approves.
        match io::stdin().lock().read_line(&mut input) {
            Ok(0) | Err(_) => return self.deny(DenialReason::NoApprovalChannel),
            Ok(_) => {}
        }

        match classify_confirmation_line(&input) {
            LineDecision::Approve => ConfirmResult::Approved,
            LineDecision::Always => {
                self.allow_list.insert(tool_name.to_string());
                ConfirmResult::Approved
            }
            LineDecision::Quit => ConfirmResult::Quit,
            LineDecision::Deny => self.deny(DenialReason::User),
        }
    }

    fn deny(&mut self, reason: DenialReason) -> ConfirmResult {
        self.denied_count += 1;
        ConfirmResult::Denied(reason)
    }

    #[cfg(test)]
    pub(crate) fn check_count(&self) -> usize {
        self.check_count
    }

    /// How many times the *interactive* path was entered. Delegated engines
    /// must keep this at zero even when they deny the tool.
    #[cfg(test)]
    pub(crate) fn prompt_count(&self) -> usize {
        self.prompt_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineDecision {
    Approve,
    Always,
    Quit,
    Deny,
}

/// The terminal prompt has no "default yes": Enter, EOF, and unknown input all
/// deny. Callers that need a default must use auto-approve/allow-list instead.
fn classify_confirmation_line(line: &str) -> LineDecision {
    match line.trim().to_lowercase().as_str() {
        "y" | "yes" => LineDecision::Approve,
        "a" | "always" => LineDecision::Always,
        "q" | "quit" => LineDecision::Quit,
        _ => LineDecision::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_approve_always_allows() {
        let mut confirmer = ToolConfirmer::new(true, vec![]);
        assert_eq!(
            confirmer.check("Bash", "echo hello"),
            ConfirmResult::Approved
        );
        assert_eq!(
            confirmer.check("Read", "/tmp/file"),
            ConfirmResult::Approved
        );
        assert_eq!(
            confirmer.check("Write", "/tmp/out"),
            ConfirmResult::Approved
        );
    }

    #[test]
    fn test_allowlist_contains_tool() {
        let mut confirmer = ToolConfirmer::new(false, vec!["Read".into(), "Write".into()]);
        assert_eq!(
            confirmer.check("Read", "/tmp/file"),
            ConfirmResult::Approved
        );
        assert_eq!(
            confirmer.check("Write", "/tmp/out"),
            ConfirmResult::Approved
        );
    }

    #[test]
    fn test_allowlist_approves_even_when_auto_approve_is_false() {
        let mut confirmer = ToolConfirmer::new(false, vec!["Read".into()]);
        assert_eq!(
            confirmer.check("Read", "/some/path"),
            ConfirmResult::Approved
        );
    }

    #[test]
    fn test_unattended_confirmer_denies_without_prompting() {
        let mut confirmer = ToolConfirmer::new(false, vec!["Read".into()]);
        confirmer.deny_unconfirmed();
        assert_eq!(
            confirmer.check("Write", "out.txt"),
            ConfirmResult::Denied(DenialReason::NoApprovalChannel)
        );
        assert_eq!(
            confirmer.check("Bash", "rm -rf ."),
            ConfirmResult::Denied(DenialReason::NoApprovalChannel)
        );
        // An explicit allow-list entry is still authoritative.
        assert_eq!(
            confirmer.check("Read", "in.txt"),
            ConfirmResult::Approved
        );
        // PR #353: the deny path must return before any stdin prompt. If it
        // ever regressed to prompting, an EOF/closed stdin would again be read
        // as an empty answer.
        assert_eq!(
            confirmer.prompt_count(),
            0,
            "deny_unconfirmed must never enter the interactive prompt path"
        );
    }

    #[test]
    fn test_auto_approve_never_prompts_on_stdin() {
        let mut confirmer = ToolConfirmer::new(true, vec![]);
        assert_eq!(
            confirmer.check("Bash", "echo hello"),
            ConfirmResult::Approved
        );
        assert_eq!(confirmer.prompt_count(), 0);
    }

    #[test]
    fn denied_count_tracks_fail_closed_denials() {
        let mut confirmer = ToolConfirmer::new(false, vec!["Read".into()]);
        confirmer.deny_unconfirmed();
        assert_eq!(confirmer.check("Write", "out.txt"), ConfirmResult::Denied(DenialReason::NoApprovalChannel));
        assert_eq!(confirmer.check("Bash", "rm -rf ."), ConfirmResult::Denied(DenialReason::NoApprovalChannel));
        assert_eq!(confirmer.check("Read", "in.txt"), ConfirmResult::Approved);
        assert_eq!(confirmer.denied_count(), 2);

        let mut auto = ToolConfirmer::new(true, vec![]);
        auto.deny_unconfirmed();
        assert_eq!(auto.check("Write", "out.txt"), ConfirmResult::Approved);
        assert_eq!(auto.denied_count(), 0);
    }

    #[test]
    fn test_empty_or_unknown_input_never_approves() {
        assert_eq!(classify_confirmation_line(""), LineDecision::Deny);
        assert_eq!(classify_confirmation_line("   "), LineDecision::Deny);
        assert_eq!(classify_confirmation_line("n"), LineDecision::Deny);
        assert_eq!(classify_confirmation_line("maybe"), LineDecision::Deny);
        assert_eq!(classify_confirmation_line("y"), LineDecision::Approve);
        assert_eq!(classify_confirmation_line(" YES "), LineDecision::Approve);
        assert_eq!(classify_confirmation_line("a"), LineDecision::Always);
        assert_eq!(classify_confirmation_line("q"), LineDecision::Quit);
    }

    #[test]
    fn denial_messages_distinguish_user_from_missing_channel() {
        assert_eq!(
            DenialReason::User.message(),
            "Tool execution denied by user"
        );
        let message = DenialReason::NoApprovalChannel.message();
        assert!(message.contains("no approval channel"));
        assert!(!message.contains("denied by user"));
    }

    // Phase 6: add_to_allow_list() grants runtime approval
    #[test]
    fn test_add_to_allow_list_grants_approval() {
        let mut confirmer = ToolConfirmer::new(false, vec![]);
        // Before: tool not in list (would prompt — skip interactive check, just verify membership)
        confirmer.add_to_allow_list("Write");
        // After: auto-approved without interactive prompt
        assert_eq!(
            confirmer.check("Write", "file.txt"),
            ConfirmResult::Approved
        );
    }

    // Phase 6: add_to_allow_list() is idempotent — adding twice has no bad effect
    #[test]
    fn test_add_to_allow_list_idempotent() {
        let mut confirmer = ToolConfirmer::new(false, vec![]);
        confirmer.add_to_allow_list("Bash");
        confirmer.add_to_allow_list("Bash"); // duplicate — HashSet, no panic
        assert_eq!(confirmer.check("Bash", "echo hi"), ConfirmResult::Approved);
    }

    // Phase 6: add_to_allow_list() does not affect unrelated tools
    #[test]
    fn test_add_to_allow_list_does_not_affect_other_tools() {
        let mut confirmer = ToolConfirmer::new(false, vec![]);
        confirmer.add_to_allow_list("Read");
        // Write is not in the list — check returns non-Approved for non-interactive
        // (we cannot test interactive input; verify Read is approved and Write is not in list)
        assert_eq!(confirmer.check("Read", "file.txt"), ConfirmResult::Approved);
        // We can't test the Denied path without stdin, but we verify allow_list state:
        assert!(confirmer.allow_list.contains("Read"));
        assert!(!confirmer.allow_list.contains("Write"));
    }
}
