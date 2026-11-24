use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillId {
    pub owner_handle: String,
    pub slug: String,
    pub version: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("skill owner handle, slug, and version are required")]
    Empty,
}

pub fn parse_skill_id(
    owner_handle: &str,
    slug: &str,
    version: &str,
) -> Result<SkillId, IdError> {
    let owner_handle =
        owner_handle.trim().trim_start_matches('@').to_lowercase();
    let slug = slug.trim();
    let version = version.trim();
    if owner_handle.is_empty() || slug.is_empty() || version.is_empty() {
        return Err(IdError::Empty);
    }
    Ok(SkillId {
        owner_handle,
        slug: slug.to_owned(),
        version: version.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_and_slug_are_required_and_normalized() {
        let id = parse_skill_id(" @Awspace ", "pdf", "1.2.3").unwrap();
        assert_eq!(id.owner_handle, "awspace");
        assert_eq!(id.slug, "pdf");
        assert_eq!(id.version, "1.2.3");
        assert!(parse_skill_id("", "pdf", "1.2.3").is_err());
        assert!(parse_skill_id("awspace", "pdf", "").is_err());
    }
}
