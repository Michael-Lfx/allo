//! Mutating-tool and verification-command heuristics for coding guards.

/// Tool names that count as workspace-mutating progress for coding guards.
pub fn is_mutating_tool(name: &str) -> bool {
    matches!(
        name,
        "Edit" | "Write" | "ApplyPatch" | "Bash" | "exec_command" | "write_stdin"
    )
}

/// Heuristic: a Bash/exec result looks like a verification command.
pub fn looks_like_verification_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "test",
        "pytest",
        "cargo test",
        "cargo check",
        "cargo build",
        "npm test",
        "npm run test",
        "pnpm test",
        "bun test",
        "go test",
        "mvn test",
        "gradle test",
        "make test",
        "make check",
        "lint",
        "eslint",
        "tsc",
        "typecheck",
        "vitest",
        "jest",
        "rspec",
        "phpunit",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cargo_test() {
        assert!(looks_like_verification_command("cargo test -p foo"));
        assert!(!looks_like_verification_command("ls -la"));
    }
}
