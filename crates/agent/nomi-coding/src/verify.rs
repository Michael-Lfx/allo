//! Mutating-tool and verification-command heuristics for coding guards.

/// Tools that *can* change the workspace (office side-effect + shell capability).
///
/// Coding **progress** does not treat non-verify `Bash`/`exec_command` as a
/// tour reset — see [`crate::progress::is_recon_tool`]. This list stays inclusive
/// so office evidence and capability checks still see the shell as mutating.
pub fn is_mutating_tool(name: &str) -> bool {
    matches!(
        name,
        "Edit" | "Write" | "ApplyPatch" | "Bash" | "exec_command" | "write_stdin"
    )
}

/// Browser / desktop / app-launch side effects (office cascade + evidence).
pub fn is_side_effect_tool(name: &str) -> bool {
    is_mutating_tool(name)
        || matches!(
            name,
            "Browser" | "Computer" | "LaunchApp" | "computer" | "browser"
        )
}

/// Isolated coding recon/verify tools — parent explore hard-stop ignores these.
pub fn is_isolated_subagent_tool(name: &str) -> bool {
    matches!(name, "explore_code" | "verify_change" | "research")
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
        "check:quick",
        "check:guards",
        "bun run check",
        "biome",
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
        assert!(looks_like_verification_command("bun run check:quick"));
        assert!(looks_like_verification_command("bunx biome check ."));
        assert!(!looks_like_verification_command("ls -la"));
    }
}
