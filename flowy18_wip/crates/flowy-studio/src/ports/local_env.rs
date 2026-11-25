//! Read-only local environment health snapshot (OS folders + L3 semantic toggle).

use super::SkillChip;
use crate::projects::{self, Project};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvSeverity {
    Ready,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalEnvDetail {
    Key(&'static str),
    Literal(String),
}

impl LocalEnvDetail {
    #[must_use]
    pub fn resolve<F: Fn(&str) -> String>(&self, t: F) -> String {
        match self {
            Self::Key(key) => t(key),
            Self::Literal(text) => text.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEnvCheck {
    pub label_key: &'static str,
    pub severity: EnvSeverity,
    pub detail: LocalEnvDetail,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEnvSnapshot {
    pub overall: EnvSeverity,
    pub semantic_verify_enabled: bool,
    pub checks: Vec<LocalEnvCheck>,
}

impl Default for LocalEnvSnapshot {
    fn default() -> Self {
        Self {
            overall: EnvSeverity::Warning,
            semantic_verify_enabled: false,
            checks: Vec::new(),
        }
    }
}

/// Evaluates lightweight local environment checks without reading any kernel
/// state files. Call on state changes or popover open — never per render frame.
#[must_use]
pub fn probe_local_env(
    projects: &[Project],
    installed_skills: &[SkillChip],
    cloud_ready: bool,
    semantic_verify_enabled: bool,
) -> LocalEnvSnapshot {
    let unavailable_projects = projects
        .iter()
        .filter(|project| !projects::root_available(&project.root))
        .count();

    let mut checks = Vec::with_capacity(4);
    checks.push(LocalEnvCheck {
        label_key: "env.check_projects",
        severity: if unavailable_projects == 0 {
            EnvSeverity::Ready
        } else {
            EnvSeverity::Warning
        },
        detail: LocalEnvDetail::Literal(if unavailable_projects == 0 {
            format!("{}/{}", projects.len(), projects.len())
        } else {
            format!(
                "{}/{} ({unavailable_projects}!)",
                projects.len().saturating_sub(unavailable_projects),
                projects.len()
            )
        }),
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_skills",
        severity: EnvSeverity::Ready,
        detail: LocalEnvDetail::Literal(installed_skills.len().to_string()),
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_model_route",
        severity: if cloud_ready {
            EnvSeverity::Ready
        } else {
            EnvSeverity::Warning
        },
        detail: LocalEnvDetail::Key(if cloud_ready {
            "env.state_ready"
        } else {
            "env.state_setup"
        }),
    });
    checks.push(LocalEnvCheck {
        label_key: "env.check_l3_verify",
        severity: EnvSeverity::Ready,
        detail: LocalEnvDetail::Key(if semantic_verify_enabled {
            "env.state_on"
        } else {
            "env.state_off"
        }),
    });

    let overall = if unavailable_projects > 0 || !cloud_ready {
        EnvSeverity::Warning
    } else {
        EnvSeverity::Ready
    };

    LocalEnvSnapshot {
        overall,
        semantic_verify_enabled,
        checks,
    }
}
