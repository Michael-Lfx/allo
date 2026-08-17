//! Structured coding-boundary error messages (not an OS sandbox).

/// Stable prefix so the model can pattern-match boundary failures.
pub const BOUNDARY_PREFIX: &str = "CODING_BOUNDARY:";

/// Write/Edit/ApplyPatch rejected because the path left write_root.
pub fn format_write_root_rejection(file_path: &str, root_display: &str) -> String {
    format!(
        "{BOUNDARY_PREFIX} Write rejected: {file_path} is outside the allowed write root {root_display}. \
         Recovery: use a path under that root (relative to the session working directory). \
         Prefer Write/Edit/ApplyPatch for file changes; use Grep with a subdirectory `path` \
         instead of scanning the whole disk."
    )
}

/// Wrap an existing capability/sandbox-style Bash error with the coding prefix
/// and a recovery suggestion. Idempotent if already prefixed.
pub fn wrap_boundary_error(detail: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.starts_with(BOUNDARY_PREFIX) {
        return trimmed.to_string();
    }
    format!(
        "{BOUNDARY_PREFIX} {trimmed} \
         Recovery: stay inside the working directory / write root; prefer Grep/Read/Glob \
         with a narrow path for discovery; use Bash for build/test/git only."
    )
}

/// Heuristic: Bash script looks like an unbounded tree walk / destructive broad op.
pub fn looks_like_broad_or_dangerous_scan(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "rm -rf /",
        "rm -rf /*",
        "find / ",
        "find / -",
        "grep -r / ",
        "rg / ",
        "del /s /q c:\\",
        "remove-item -recurse -force c:\\",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

/// Message when coding boundary mode rejects a broad/dangerous Bash command.
pub fn format_broad_scan_rejection(command_preview: &str) -> String {
    let preview: String = command_preview.chars().take(120).collect();
    format!(
        "{BOUNDARY_PREFIX} Refused broad or dangerous command in coding mode: {preview:?}. \
         Recovery: scope discovery with Grep/Glob (`path` = subdirectory) or a narrower shell cwd; \
         do not scan or mutate the whole filesystem."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_root_message_has_prefix() {
        let msg = format_write_root_rejection("/tmp/x", "/ws");
        assert!(msg.starts_with(BOUNDARY_PREFIX));
        assert!(msg.contains("/ws"));
    }

    #[test]
    fn wrap_is_idempotent() {
        let once = wrap_boundary_error("denied");
        let twice = wrap_boundary_error(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn detects_broad_find() {
        assert!(looks_like_broad_or_dangerous_scan("find / -name '*.rs'"));
        assert!(!looks_like_broad_or_dangerous_scan("cargo test -p foo"));
    }
}
