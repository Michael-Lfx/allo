//! Tool Policy Engine - Structured policy hard engine for tool execution security.
//!
//! Authoritative baselines:
//! - `docs/architecture/voice-agent/interaction.zh.md` §2.5, §3.7
//! - `docs/architecture/voice-agent/execution/wave-3-agent-safety.zh.md` (T3-03, T3-05)

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

/// Error types for confirmation token lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum ConfirmationError {
    #[error("Confirmation token not found: {0}")]
    NotFound(String),
    #[error("Confirmation token expired (TTL exceeded 8s)")]
    Expired,
    #[error("Confirmation token has already been consumed (single-use constraint)")]
    AlreadyConsumed,
    #[error("Confirmation token task mismatch: expected {expected}, got {actual}")]
    TaskMismatch { expected: String, actual: String },
}

/// Single-use confirmation token with 8-second TTL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationToken {
    pub token_id: String,
    pub task_id: String,
    pub tool_call_id: String,
    pub action_summary: String,
    #[serde(skip, default = "Instant::now")]
    pub created_at: Instant,
    pub ttl_secs: u64,
    pub consumed: bool,
}

/// Interception reason category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InterceptReason {
    DestructiveCommand {
        command: String,
        matched_pattern: String,
    },
    SandboxPathEscape {
        path: String,
        sandbox_root: String,
    },
    DestructiveFileAction {
        path: String,
        action: String,
    },
    DestructiveDatabaseAction {
        query: String,
        matched_keyword: String,
    },
    UnknownToolFailSafe {
        tool_name: String,
    },
    InvalidArguments {
        details: String,
    },
}

/// Policy evaluation verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum PolicyVerdict {
    Allowed,
    SuspendedAwaitingConfirmation {
        token: ConfirmationToken,
        reason: InterceptReason,
        prompt: String,
    },
    Blocked {
        reason: InterceptReason,
    },
}

/// Structured request for a tool call execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub task_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// Frozen schema configuration for the Tool Policy Table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolPolicyTable {
    pub sandbox_root: PathBuf,
    pub fail_safe_deny: bool,
    pub blocked_commands: Vec<String>,
    pub blocked_sql_keywords: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_commands: Vec<String>,
}

impl Default for ToolPolicyTable {
    fn default() -> Self {
        Self {
            sandbox_root: PathBuf::from("/var/lib/ai-mate/sandbox"),
            fail_safe_deny: true,
            blocked_commands: vec![
                "rm".to_string(),
                "rmdir".to_string(),
                "unlink".to_string(),
                "reboot".to_string(),
                "shutdown".to_string(),
                "poweroff".to_string(),
                "halt".to_string(),
                "mkfs".to_string(),
                "drop".to_string(),
                "truncate".to_string(),
                "kill".to_string(),
                "killall".to_string(),
                "pkill".to_string(),
                "dd".to_string(),
                "format".to_string(),
                "fdisk".to_string(),
                "parted".to_string(),
            ],
            blocked_sql_keywords: vec![
                "drop".to_string(),
                "truncate".to_string(),
                "alter".to_string(),
                "delete".to_string(),
            ],
            allowed_tools: vec![
                "bash_run".to_string(),
                "file_read".to_string(),
                "file_write".to_string(),
                "file_delete".to_string(),
                "file_list".to_string(),
                "db_query".to_string(),
            ],
            allowed_commands: vec![
                "ls".to_string(),
                "pwd".to_string(),
                "cat".to_string(),
                "echo".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "stat".to_string(),
                "date".to_string(),
                "whoami".to_string(),
                "uptime".to_string(),
                "git".to_string(),
                "df".to_string(),
                "free".to_string(),
                "ps".to_string(),
                "wc".to_string(),
                "head".to_string(),
                "tail".to_string(),
            ],
        }
    }
}

// ----------------------------------------------------------------------------
// Path and command analysis helpers
// ----------------------------------------------------------------------------

/// Pure lexical path normalization without disk I/O.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if let Some(last) = components.last() {
                    if !matches!(last, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
                        components.pop();
                        continue;
                    }
                }
                components.push(comp);
            }
            _ => components.push(comp),
        }
    }
    components.into_iter().collect()
}

/// Checks whether a given path is strictly within the sandbox boundary.
pub fn is_path_inside_sandbox(path_str: &str, sandbox_root: &Path) -> bool {
    // Windows and Unix path slash unification
    let p = Path::new(path_str);
    let normalized = if p.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
        lexical_normalize(p)
    } else {
        lexical_normalize(&sandbox_root.join(p))
    };

    let norm_sandbox = lexical_normalize(sandbox_root);

    let norm_str = normalized.to_string_lossy().replace('\\', "/");
    let sandbox_str = norm_sandbox.to_string_lossy().replace('\\', "/");
    let sandbox_prefix = if sandbox_str.ends_with('/') {
        sandbox_str
    } else {
        format!("{sandbox_str}/")
    };

    norm_str == norm_sandbox.to_string_lossy().replace('\\', "/") || norm_str.starts_with(&sandbox_prefix)
}

/// Recursively inspects a command string for blocked command invocations and injection tricks.
pub fn inspect_command_string(cmd: &str, blocked_commands: &[String]) -> Option<String> {
    // 1. Detect subshells: $(...) or `...`
    if let Some(sub) = extract_subshell(cmd) {
        if let Some(matched) = inspect_command_string(&sub, blocked_commands) {
            return Some(matched);
        }
    }

    // 2. Split into pipeline and chain segments outside quotes
    let segments = split_command_segments(cmd);
    for seg in segments {
        let trimmed = seg.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Split segment into words
        let words = split_words(trimmed);
        if words.is_empty() {
            continue;
        }

        let mut idx = 0;
        // Skip leading env assignments like VAR=value
        while idx < words.len() && words[idx].contains('=') && !words[idx].starts_with('-') {
            idx += 1;
        }
        if idx >= words.len() {
            continue;
        }

        let raw_exe = &words[idx];
        let cleaned_exe = clean_executable_name(raw_exe);

        // Check if executable is in blocked list
        for blocked in blocked_commands {
            if cleaned_exe.eq_ignore_ascii_case(blocked)
                || cleaned_exe.starts_with(&format!("{blocked}."))
                || cleaned_exe.starts_with(&format!("{blocked}-"))
            {
                return Some(cleaned_exe);
            }
        }

        // Wrapper command handling
        match cleaned_exe.as_str() {
            "sudo" | "doas" | "nohup" | "nice" | "env" => {
                let remainder = words[idx + 1..].join(" ");
                if let Some(matched) = inspect_command_string(&remainder, blocked_commands) {
                    return Some(matched);
                }
            }
            "xargs" => {
                // xargs <command>
                let remainder = words[idx + 1..].join(" ");
                if let Some(matched) = inspect_command_string(&remainder, blocked_commands) {
                    return Some(matched);
                }
            }
            "find" => {
                // Check for find -delete or find ... -exec <cmd>
                for (w_idx, w) in words.iter().enumerate() {
                    if w == "-delete" {
                        return Some("find -delete".to_string());
                    }
                    if w == "-exec" && w_idx + 1 < words.len() {
                        let exec_cmd = words[w_idx + 1..].join(" ");
                        if let Some(matched) = inspect_command_string(&exec_cmd, blocked_commands) {
                            return Some(matched);
                        }
                    }
                }
            }
            "sh" | "bash" | "zsh" | "dash" | "ksh" => {
                // sh -c "command"
                if let Some(c_idx) = words.iter().position(|w| w == "-c") {
                    if c_idx + 1 < words.len() {
                        let inner = words[c_idx + 1..].join(" ");
                        if let Some(matched) = inspect_command_string(&inner, blocked_commands) {
                            return Some(matched);
                        }
                    }
                }
            }
            "eval" | "exec" => {
                let remainder = words[idx + 1..].join(" ");
                if let Some(matched) = inspect_command_string(&remainder, blocked_commands) {
                    return Some(matched);
                }
            }
            _ => {}
        }

        // Prompt injection defense: scan words in segment for explicit destructive calls
        for (i, word) in words.iter().enumerate() {
            let cl = clean_executable_name(word);
            for blocked in blocked_commands {
                if cl.eq_ignore_ascii_case(blocked) && (i == 0 || words[i - 1] == ";" || words[i - 1] == "&&" || words[i - 1] == "||" || words[i - 1] == ":" || words[i - 1] == "-exec") {
                    return Some(cl.clone());
                }
            }
        }
    }

    // Direct search for prompt injection patterns like "rm -rf" or "reboot" in unquoted fragments
    let lower = cmd.to_ascii_lowercase();
    for blocked in blocked_commands {
        // Look for occurrences of word boundaries with destructive arguments
        let blocked_pattern = format!(" {blocked} ");
        let blocked_start = format!("{blocked} ");
        let blocked_flag = format!(" {blocked} -");
        let blocked_end = format!(" {blocked}");
        if lower.contains(&blocked_pattern)
            || lower.starts_with(&blocked_start)
            || lower.contains(&blocked_flag)
            || lower.ends_with(&blocked_end)
        {
            // Verify it is not inside an echo or grep benign argument
            let words = split_words(cmd);
            if let Some(first) = words.first() {
                let first_clean = clean_executable_name(first);
                if first_clean == "echo" || first_clean == "grep" {
                    // Benign invocation where blocked word is inside string parameter
                    continue;
                }
            }
            return Some(blocked.clone());
        }
    }

    None
}

fn extract_subshell(cmd: &str) -> Option<String> {
    if let Some(start) = cmd.find("$(") {
        if let Some(end) = cmd[start..].find(')') {
            return Some(cmd[start + 2..start + end].to_string());
        }
    }
    if let Some(start) = cmd.find('`') {
        if let Some(end) = cmd[start + 1..].find('`') {
            return Some(cmd[start + 1..start + 1 + end].to_string());
        }
    }
    None
}

fn split_command_segments(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_double {
            in_single = !in_single;
            current.push(c);
        } else if c == '"' && !in_single {
            in_double = !in_double;
            current.push(c);
        } else if !in_single && !in_double {
            if c == ';' || c == '\n' || c == '\r' {
                segments.push(current.trim().to_string());
                current.clear();
            } else if c == '&' && i + 1 < chars.len() && chars[i + 1] == '&' {
                segments.push(current.trim().to_string());
                current.clear();
                i += 1;
            } else if c == '|' && i + 1 < chars.len() && chars[i + 1] == '|' {
                segments.push(current.trim().to_string());
                current.clear();
                i += 1;
            } else if c == '|' {
                segments.push(current.trim().to_string());
                current.clear();
            } else {
                current.push(c);
            }
        } else {
            current.push(c);
        }
        i += 1;
    }

    if !current.trim().is_empty() {
        segments.push(current.trim().to_string());
    }

    segments
}

fn split_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in s.chars() {
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        } else if c.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                words.push(current);
                current = String::new();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn clean_executable_name(raw: &str) -> String {
    let s = raw.trim().trim_matches(|c| c == '\'' || c == '"' || c == '(' || c == ')' || c == '{' || c == '}');
    // Remove directory prefixes
    let path = Path::new(s);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(s);
    let cleaned = file_name.trim_end_matches(".exe");
    cleaned.replace('\\', "")
}

/// Inspects SQL statements for blocked keywords.
pub fn inspect_sql_query(query: &str, blocked_keywords: &[String]) -> Option<String> {
    let statements: Vec<&str> = query.split(';').collect();
    for stmt in statements {
        let words = split_words(stmt);
        for word in words {
            let clean_word = word.trim_matches(|c| c == '\'' || c == '"' || c == '`' || c == ';');
            for blocked in blocked_keywords {
                if clean_word.eq_ignore_ascii_case(blocked) {
                    return Some(clean_word.to_ascii_uppercase());
                }
            }
        }
    }
    None
}

// ----------------------------------------------------------------------------
// Tool Policy Engine Implementation
// ----------------------------------------------------------------------------

pub struct ToolPolicyEngine {
    policy: ToolPolicyTable,
    tokens: HashMap<String, ConfirmationToken>,
    suspended_calls: HashMap<String, ToolCallRequest>,
}

impl ToolPolicyEngine {
    pub const DEFAULT_TTL: Duration = Duration::from_secs(8);

    pub fn new(policy: ToolPolicyTable) -> Self {
        Self {
            policy,
            tokens: HashMap::new(),
            suspended_calls: HashMap::new(),
        }
    }

    pub fn default_ai_mate() -> Self {
        Self::new(ToolPolicyTable::default())
    }

    pub fn policy(&self) -> &ToolPolicyTable {
        &self.policy
    }

    /// Evaluates a tool call request against the structured policy table and sandboxing rules.
    pub fn evaluate(&mut self, request: &ToolCallRequest) -> PolicyVerdict {
        // 0. Periodic memory GC: prune expired tokens and orphaned suspended calls
        self.prune_expired();

        // 1. Fail-safe default deny: unknown tool
        if !self.policy.allowed_tools.iter().any(|t| t == &request.tool_name) {
            let reason = InterceptReason::UnknownToolFailSafe {
                tool_name: request.tool_name.clone(),
            };
            return PolicyVerdict::Blocked { reason };
        }

        // 2. Dispatch inspection based on tool category
        match request.tool_name.as_str() {
            "bash_run" => self.evaluate_bash_run(request),
            "file_read" | "file_write" | "file_list" => self.evaluate_file_access(request),
            "file_delete" => self.evaluate_file_delete(request),
            "db_query" => self.evaluate_db_query(request),
            _ => {
                if self.policy.fail_safe_deny {
                    PolicyVerdict::Blocked {
                        reason: InterceptReason::UnknownToolFailSafe {
                            tool_name: request.tool_name.clone(),
                        },
                    }
                } else {
                    PolicyVerdict::Allowed
                }
            }
        }
    }

    fn evaluate_bash_run(&mut self, request: &ToolCallRequest) -> PolicyVerdict {
        let cmd = match request.arguments.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return PolicyVerdict::Blocked {
                    reason: InterceptReason::InvalidArguments {
                        details: "Missing 'command' string argument in bash_run".to_string(),
                    },
                };
            }
        };

        // Inspect for destructive commands and prompt injection variants
        if let Some(matched) = inspect_command_string(cmd, &self.policy.blocked_commands) {
            let reason = InterceptReason::DestructiveCommand {
                command: cmd.to_string(),
                matched_pattern: matched,
            };
            let prompt = format!("即将执行高危指令操作: {cmd}，确认请说确认");
            return self.suspend_and_issue_token(request, reason, prompt);
        }

        // Benign command passed policy check
        PolicyVerdict::Allowed
    }

    fn evaluate_file_access(&mut self, request: &ToolCallRequest) -> PolicyVerdict {
        let path_str = match request.arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return PolicyVerdict::Blocked {
                    reason: InterceptReason::InvalidArguments {
                        details: "Missing 'path' string argument in file operation".to_string(),
                    },
                };
            }
        };

        if !is_path_inside_sandbox(path_str, &self.policy.sandbox_root) {
            let reason = InterceptReason::SandboxPathEscape {
                path: path_str.to_string(),
                sandbox_root: self.policy.sandbox_root.to_string_lossy().to_string(),
            };
            let prompt = format!("即将访问沙箱外部文件: {path_str}，确认请说确认");
            return self.suspend_and_issue_token(request, reason, prompt);
        }

        PolicyVerdict::Allowed
    }

    fn evaluate_file_delete(&mut self, request: &ToolCallRequest) -> PolicyVerdict {
        let path_str = match request.arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return PolicyVerdict::Blocked {
                    reason: InterceptReason::InvalidArguments {
                        details: "Missing 'path' string argument in file_delete".to_string(),
                    },
                };
            }
        };

        // Path sandbox check first
        if !is_path_inside_sandbox(path_str, &self.policy.sandbox_root) {
            let reason = InterceptReason::SandboxPathEscape {
                path: path_str.to_string(),
                sandbox_root: self.policy.sandbox_root.to_string_lossy().to_string(),
            };
            let prompt = format!("即将删除沙箱外部文件: {path_str}，确认请说确认");
            return self.suspend_and_issue_token(request, reason, prompt);
        }

        // Even inside sandbox, file deletion requires oral confirmation (TC-AM-008)
        let reason = InterceptReason::DestructiveFileAction {
            path: path_str.to_string(),
            action: "delete".to_string(),
        };
        let prompt = format!("即将执行文件删除操作: {path_str}，确认请说确认");
        self.suspend_and_issue_token(request, reason, prompt)
    }

    fn evaluate_db_query(&mut self, request: &ToolCallRequest) -> PolicyVerdict {
        let query = match request.arguments.get("query").and_then(|v| v.as_str()) {
            Some(q) => q,
            None => {
                return PolicyVerdict::Blocked {
                    reason: InterceptReason::InvalidArguments {
                        details: "Missing 'query' string argument in db_query".to_string(),
                    },
                };
            }
        };

        if let Some(matched) = inspect_sql_query(query, &self.policy.blocked_sql_keywords) {
            let reason = InterceptReason::DestructiveDatabaseAction {
                query: query.to_string(),
                matched_keyword: matched,
            };
            let prompt = format!("即将执行破坏性数据库操作: {query}，确认请说确认");
            return self.suspend_and_issue_token(request, reason, prompt);
        }

        PolicyVerdict::Allowed
    }

    fn suspend_and_issue_token(
        &mut self,
        request: &ToolCallRequest,
        reason: InterceptReason,
        prompt: String,
    ) -> PolicyVerdict {
        let token_id = format!("tok_{}", uuid::Uuid::new_v4().simple());
        let token = ConfirmationToken {
            token_id: token_id.clone(),
            task_id: request.task_id.clone(),
            tool_call_id: request.tool_call_id.clone(),
            action_summary: prompt.clone(),
            created_at: Instant::now(),
            ttl_secs: 8,
            consumed: false,
        };

        self.tokens.insert(token_id.clone(), token.clone());
        self.suspended_calls.insert(token_id, request.clone());

        PolicyVerdict::SuspendedAwaitingConfirmation {
            token,
            reason,
            prompt,
        }
    }

    /// Single-use consumption of confirmation token with 8-second TTL enforcement.
    pub fn consume_token(&mut self, token_id: &str) -> Result<ConfirmationToken, ConfirmationError> {
        let token = self.tokens.get_mut(token_id).ok_or_else(|| {
            ConfirmationError::NotFound(token_id.to_string())
        })?;

        if token.consumed {
            return Err(ConfirmationError::AlreadyConsumed);
        }

        if token.created_at.elapsed() > Duration::from_secs(token.ttl_secs) {
            return Err(ConfirmationError::Expired);
        }

        token.consumed = true;
        Ok(token.clone())
    }

    /// Consumes the token and retrieves the suspended tool call for execution resumption.
    pub fn resume_call(&mut self, token_id: &str) -> Result<ToolCallRequest, ConfirmationError> {
        self.consume_token(token_id)?;
        self.suspended_calls.remove(token_id).ok_or_else(|| {
            ConfirmationError::NotFound(token_id.to_string())
        })
    }

    pub fn revoke_token(&mut self, token_id: &str) -> bool {
        self.suspended_calls.remove(token_id);
        self.tokens.remove(token_id).is_some()
    }

    pub fn is_token_expired(&self, token_id: &str) -> bool {
        match self.tokens.get(token_id) {
            Some(t) => t.created_at.elapsed() > Duration::from_secs(t.ttl_secs),
            None => true,
        }
    }

    pub fn suspended_call(&self, token_id: &str) -> Option<&ToolCallRequest> {
        self.suspended_calls.get(token_id)
    }

    /// Prunes expired confirmation tokens and orphaned suspended calls to prevent memory leaks in daemon mode.
    pub fn prune_expired(&mut self) {
        let now = Instant::now();
        self.tokens
            .retain(|_, t| now.duration_since(t.created_at) <= Duration::from_secs(t.ttl_secs));
        self.suspended_calls
            .retain(|token_id, _| self.tokens.contains_key(token_id));
    }

    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    pub fn suspended_call_count(&self) -> usize {
        self.suspended_calls.len()
    }
}

// ============================================================================
// T3-05: Task Accepted Receipt Sequence Guarantee
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDispatchStatus {
    Accepted,
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDispatchReceipt {
    pub task_id: String,
    pub status: TaskDispatchStatus,
    pub accepted_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDispatchEvent {
    TaskSubmitted { task_id: String, timestamp: Instant },
    TaskAccepted { task_id: String, timestamp: Instant },
    TaskRejected { task_id: String, reason: String, timestamp: Instant },
    SpokenAckBroadcast { task_id: String, spoken_ack: String, timestamp: Instant },
    ErrorAudioFeedback { task_id: String, timestamp: Instant },
}

pub trait TaskQueueBackend: Send + Sync {
    fn submit_task(&mut self, task_id: &str, goal: &str) -> Result<(), String>;
}

/// Task Dispatch Coordinator ensuring spoken_ack causal invariant (Baseline §2.5).
pub struct TaskDispatchCoordinator {
    event_log: Vec<TaskDispatchEvent>,
}

impl TaskDispatchCoordinator {
    pub fn new() -> Self {
        Self {
            event_log: Vec::new(),
        }
    }

    pub fn event_history(&self) -> &[TaskDispatchEvent] {
        &self.event_log
    }

    /// Dispatches a task to the backend queue and guarantees that spoken_ack
    /// is ONLY broadcast after the underlying task queue successfully confirms Accepted.
    pub fn dispatch_task(
        &mut self,
        task_id: impl Into<String>,
        goal: impl Into<String>,
        spoken_ack: Option<String>,
        queue: &mut dyn TaskQueueBackend,
    ) -> Result<TaskDispatchReceipt, String> {
        let task_id = task_id.into();
        let goal = goal.into();

        // 1. Record task submission
        let t_submit = Instant::now();
        self.event_log.push(TaskDispatchEvent::TaskSubmitted {
            task_id: task_id.clone(),
            timestamp: t_submit,
        });

        // 2. Submit to underlying task queue
        match queue.submit_task(&task_id, &goal) {
            Ok(()) => {
                let t_accept = Instant::now();
                self.event_log.push(TaskDispatchEvent::TaskAccepted {
                    task_id: task_id.clone(),
                    timestamp: t_accept,
                });

                // 3. Spoken ACK is broadcast strictly AFTER TaskAccepted event is established
                if let Some(ack) = spoken_ack {
                    let t_ack = Instant::now();
                    self.event_log.push(TaskDispatchEvent::SpokenAckBroadcast {
                        task_id: task_id.clone(),
                        spoken_ack: ack,
                        timestamp: t_ack,
                    });
                }

                Ok(TaskDispatchReceipt {
                    task_id,
                    status: TaskDispatchStatus::Accepted,
                    accepted_at: Some(t_accept),
                })
            }
            Err(reason) => {
                let t_reject = Instant::now();
                self.event_log.push(TaskDispatchEvent::TaskRejected {
                    task_id: task_id.clone(),
                    reason: reason.clone(),
                    timestamp: t_reject,
                });

                // On rejection, spoken_ack is NEVER broadcast; play error earcon instead
                let t_err = Instant::now();
                self.event_log.push(TaskDispatchEvent::ErrorAudioFeedback {
                    task_id: task_id.clone(),
                    timestamp: t_err,
                });

                Ok(TaskDispatchReceipt {
                    task_id,
                    status: TaskDispatchStatus::Rejected { reason },
                    accepted_at: None,
                })
            }
        }
    }

    /// Mathematically verifies temporal causality invariants over the event log:
    /// - SpokenAckBroadcast MUST be preceded by TaskAccepted for the same task_id.
    /// - Timestamp of TaskAccepted <= Timestamp of SpokenAckBroadcast.
    /// - No SpokenAckBroadcast may exist for any rejected task.
    pub fn verify_sequence_invariants(&self) -> Result<(), String> {
        let mut accepted_times: HashMap<String, Instant> = HashMap::new();
        let mut rejected_tasks: HashMap<String, Instant> = HashMap::new();

        for event in &self.event_log {
            match event {
                TaskDispatchEvent::TaskAccepted { task_id, timestamp } => {
                    accepted_times.insert(task_id.clone(), *timestamp);
                }
                TaskDispatchEvent::TaskRejected { task_id, timestamp, .. } => {
                    rejected_tasks.insert(task_id.clone(), *timestamp);
                }
                TaskDispatchEvent::SpokenAckBroadcast { task_id, timestamp, .. } => {
                    if rejected_tasks.contains_key(task_id) {
                        return Err(format!(
                            "Causality Violation: task '{task_id}' was rejected but produced SpokenAckBroadcast!"
                        ));
                    }
                    match accepted_times.get(task_id) {
                        Some(&t_accept) => {
                            if *timestamp < t_accept {
                                return Err(format!(
                                    "Temporal Invariant Violation: SpokenAckBroadcast ({timestamp:?}) occurred BEFORE TaskAccepted ({t_accept:?})!"
                                ));
                            }
                        }
                        None => {
                            return Err(format!(
                                "Causality Violation: SpokenAckBroadcast emitted without prior TaskAccepted for task '{task_id}'!"
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

impl Default for TaskDispatchCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_lexical_normalize_and_sandbox_boundaries() {
        let sandbox = Path::new("/var/lib/ai-mate/sandbox");

        assert!(is_path_inside_sandbox("/var/lib/ai-mate/sandbox", sandbox));
        assert!(is_path_inside_sandbox("/var/lib/ai-mate/sandbox/file.txt", sandbox));
        assert!(is_path_inside_sandbox("sub/file.txt", sandbox));
        assert!(is_path_inside_sandbox("/var/lib/ai-mate/sandbox/a/b/c/../../b/c", sandbox));

        assert!(!is_path_inside_sandbox("/etc/passwd", sandbox));
        assert!(!is_path_inside_sandbox("../../etc/shadow", sandbox));
        assert!(!is_path_inside_sandbox("/var/lib/ai-mate/sandbox/../../etc/passwd", sandbox));
        assert!(!is_path_inside_sandbox("/var/lib/ai-mate/sandbox/..\\escape.txt", sandbox));
    }

    #[test]
    fn test_token_expiration_and_revocation() {
        let mut engine = ToolPolicyEngine::default_ai_mate();
        let req = ToolCallRequest {
            task_id: "task_exp".to_string(),
            tool_call_id: "call_exp".to_string(),
            tool_name: "bash_run".to_string(),
            arguments: json!({ "command": "reboot" }),
        };

        let verdict = engine.evaluate(&req);
        let token_id = match verdict {
            PolicyVerdict::SuspendedAwaitingConfirmation { token, .. } => token.token_id,
            _ => panic!("Expected SuspendedAwaitingConfirmation"),
        };

        assert!(!engine.is_token_expired(&token_id));
        assert!(engine.suspended_call(&token_id).is_some());

        // Revoke token
        assert!(engine.revoke_token(&token_id));
        assert!(engine.suspended_call(&token_id).is_none());
        assert_eq!(
            engine.consume_token(&token_id),
            Err(ConfirmationError::NotFound(token_id))
        );
    }

    #[test]
    fn test_prune_expired_removes_dead_tokens_and_suspended_calls() {
        let mut engine = ToolPolicyEngine::default_ai_mate();
        let req = ToolCallRequest {
            task_id: "task_gc".to_string(),
            tool_call_id: "call_gc".to_string(),
            tool_name: "bash_run".to_string(),
            arguments: json!({ "command": "rm -rf /tmp/data" }),
        };

        let verdict = engine.evaluate(&req);
        let token_id = match verdict {
            PolicyVerdict::SuspendedAwaitingConfirmation { token, .. } => token.token_id,
            _ => panic!("Expected SuspendedAwaitingConfirmation"),
        };

        assert_eq!(engine.token_count(), 1);
        assert_eq!(engine.suspended_call_count(), 1);

        // Manually adjust token timestamp back by 10s to simulate timeout
        if let Some(token) = engine.tokens.get_mut(&token_id) {
            token.created_at = std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(10))
                .unwrap_or(token.created_at);
        }

        // Run GC
        engine.prune_expired();
        assert_eq!(engine.token_count(), 0);
        assert_eq!(engine.suspended_call_count(), 0);
        assert!(engine.suspended_call(&token_id).is_none());
    }
}
