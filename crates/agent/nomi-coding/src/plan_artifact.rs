//! Structured plan submitted when leaving plan mode.

use serde::{Deserialize, Serialize};

/// Versioned implementation plan the execute phase can hang receipts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanArtifact {
    pub version: u32,
    pub goal: String,
    pub scope: Vec<String>,
    pub verify_commands: Vec<String>,
    pub body: String,
}

impl PlanArtifact {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn from_exit_content(content: &str) -> Self {
        let body = content.trim().to_string();
        let goal = body
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("implementation plan")
            .trim_start_matches(['#', '-', '*', ' '])
            .to_string();
        let verify_commands = body
            .lines()
            .filter(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("test")
                    || lower.contains("lint")
                    || lower.contains("cargo ")
                    || lower.contains("npm ")
            })
            .map(|line| line.trim().to_string())
            .take(8)
            .collect();
        let scope = body
            .lines()
            .filter(|line| {
                line.contains('/') || line.ends_with(".rs") || line.ends_with(".ts")
            })
            .map(|line| line.trim().to_string())
            .take(16)
            .collect();
        Self {
            version: Self::CURRENT_VERSION,
            goal,
            scope,
            verify_commands,
            body,
        }
    }

    pub fn summary_block(&self) -> String {
        let mut out = format!(
            "[PlanArtifact v{}]\nGoal: {}\n",
            self.version, self.goal
        );
        if !self.scope.is_empty() {
            out.push_str("Scope:\n");
            for item in &self.scope {
                out.push_str(&format!("- {item}\n"));
            }
        }
        if !self.verify_commands.is_empty() {
            out.push_str("Verify:\n");
            for cmd in &self.verify_commands {
                out.push_str(&format!("- {cmd}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_goal_and_verify_lines() {
        let artifact = PlanArtifact::from_exit_content(
            "# Fix login\n- touch src/auth.rs\n- cargo test -p app\n",
        );
        assert_eq!(artifact.version, 1);
        assert!(artifact.goal.contains("Fix login"));
        assert!(artifact.verify_commands.iter().any(|c| c.contains("cargo test")));
    }
}
