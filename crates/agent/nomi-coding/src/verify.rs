//! Mutating-tool and verification-command heuristics for coding guards.

use serde_json::Value;

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

/// Shell text from a tool's JSON arguments.
///
/// `Bash` uses `command`. `exec_command` uses `cmd` in command mode and
/// `script` in script mode. A progress guard that only reads `command` will
/// treat every `exec_command` as recon — that is how session
/// `01a0d19b-c5e8-7381-a811-a7b715bf058a` classified `go run` / `pnpm dev`
/// as an exploration tour.
pub fn shell_command_from_input(input: &Value) -> Option<String> {
    const KEYS: &[&str] = &["command", "cmd", "script"];
    for key in KEYS {
        if let Some(text) = input.get(*key).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// Heuristic: a Bash/exec result is a world-changing action, not a file tour.
///
/// `ls` / `git status` / `cat` stay recon. Installs, process starts, file
/// writes, and HTTP probes are the work of "start this project" tasks and
/// must reset the explore streak.
pub fn looks_like_progress_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "pnpm install",
        "npm install",
        "yarn install",
        "bun install",
        "pnpm i ",
        "npm i ",
        "pnpm --filter",
        "pnpm dev",
        "npm run dev",
        "npm start",
        "bun run dev",
        "go run",
        "cargo run",
        "set-content",
        "out-file",
        "add-content",
        "invoke-webrequest",
        "invoke-restmethod",
        "curl ",
        "wget ",
        "start-process",
        "docker compose up",
        "docker-compose up",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
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
        "go build",
        "go vet",
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
        assert!(looks_like_verification_command("go build ./..."));
        assert!(looks_like_verification_command("go vet ./internal/..."));
        assert!(!looks_like_verification_command("ls -la"));
    }

    #[test]
    fn shell_command_reads_exec_command_cmd() {
        let bash = serde_json::json!({"command": "ls -la"});
        assert_eq!(shell_command_from_input(&bash).as_deref(), Some("ls -la"));
        let exec = serde_json::json!({
            "cmd": "go run ./cmd/server",
            "yield_time_ms": 25000
        });
        assert_eq!(
            shell_command_from_input(&exec).as_deref(),
            Some("go run ./cmd/server")
        );
        let script = serde_json::json!({"script": "print('hi')"});
        assert_eq!(
            shell_command_from_input(&script).as_deref(),
            Some("print('hi')")
        );
        assert_eq!(shell_command_from_input(&serde_json::json!({})), None);
    }

    #[test]
    fn progress_commands_match_start_project_session() {
        assert!(looks_like_progress_command(
            "pnpm install --frozen-lockfile 2>&1 | Select-Object -Last 15"
        ));
        assert!(looks_like_progress_command(
            "Set-Location C:/Users/scx/Desktop/saas/backend; go run ./cmd/server"
        ));
        assert!(looks_like_progress_command(
            "pnpm --filter @aics/web dev --port 5175 --strictPort"
        ));
        assert!(looks_like_progress_command(
            "Invoke-WebRequest -Uri 'http://localhost:8080/healthz'"
        ));
        assert!(!looks_like_progress_command("ls -la"));
        assert!(!looks_like_progress_command("git status"));
        assert!(!looks_like_progress_command("go version"));
        assert!(!looks_like_progress_command("Test-Path web/node_modules"));
    }
}
