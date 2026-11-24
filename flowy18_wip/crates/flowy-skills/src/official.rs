use crate::{IdError, SkillId, parse_skill_id};
use serde::Deserialize;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfficialSkill {
    pub id: SkillId,
    pub title: String,
    pub summary: String,
    pub auto_invoke: bool,
}

#[derive(Debug, Error)]
pub enum OfficialError {
    #[error("official skill catalog is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid official skill identity: {0}")]
    Id(#[from] IdError),
}

#[derive(Deserialize)]
struct OfficialRecord {
    owner_handle: String,
    slug: String,
    version: String,
    title: String,
    summary: String,
    auto_invoke: bool,
}

pub fn bundled_official() -> Result<Vec<OfficialSkill>, OfficialError> {
    parse_official(include_str!("../assets/official-skills.json"))
}

pub fn parse_official(body: &str) -> Result<Vec<OfficialSkill>, OfficialError> {
    let records: Vec<OfficialRecord> = serde_json::from_str(body)?;
    records
        .into_iter()
        .map(|record| {
            Ok(OfficialSkill {
                id: parse_skill_id(
                    &record.owner_handle,
                    &record.slug,
                    &record.version,
                )?,
                title: record.title,
                summary: record.summary,
                auto_invoke: record.auto_invoke,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_contains_excel_and_allows_auto_invoke() {
        let skills = bundled_official().unwrap();
        let excel = skills
            .iter()
            .find(|skill| skill.id.slug == "excel")
            .unwrap();
        assert_eq!(excel.id.owner_handle, "flowy");
        assert!(excel.auto_invoke);
    }

    #[test]
    fn malformed_official_entry_fails_the_whole_catalog() {
        let body = r#"
        [
          {
            "owner_handle": "flowy",
            "slug": "excel",
            "title": "Excel",
            "summary": "Spreadsheets",
            "auto_invoke": true
          }
        ]
        "#;
        assert!(parse_official(body).is_err());
    }
}
