//! Session task profiles: office (general desktop) vs coding overlay.

use serde::{Deserialize, Serialize};

/// User-selectable session work mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskProfile {
    /// General desktop / office automation (default).
    #[default]
    Office,
    /// Coding overlay: tighter tools, coding guidance, stronger stop/verify.
    Coding,
}

impl TaskProfile {
    pub const OFFICE: &'static str = "office";
    pub const CODING: &'static str = "coding";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Office => Self::OFFICE,
            Self::Coding => Self::CODING,
        }
    }

    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some(Self::CODING) | Some("code") | Some("dev") | Some("development") => Self::Coding,
            _ => Self::Office,
        }
    }

    pub fn is_coding(self) -> bool {
        matches!(self, Self::Coding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_to_office() {
        assert_eq!(TaskProfile::parse(None), TaskProfile::Office);
        assert_eq!(TaskProfile::parse(Some("office")), TaskProfile::Office);
        assert_eq!(TaskProfile::parse(Some("coding")), TaskProfile::Coding);
        assert_eq!(TaskProfile::parse(Some("CODE")), TaskProfile::Coding);
    }
}
