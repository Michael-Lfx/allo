use thiserror::Error;

pub const CATEGORY_TTL_SECS: u64 = 86_400;
pub const SEARCH_TTL_SECS: u64 = 900;
pub const PAGE_LIMIT_CATEGORY: u32 = 20;
pub const PAGE_LIMIT_SEARCH: u32 = 15;
pub const MAX_PACKAGE_BYTES: u64 = 8_388_608;

const ORIGIN: &str = "https://clawhub.ai";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetch {
    Trending,
    Category {
        slug: String,
    },
    Search {
        query: String,
        category: Option<String>,
    },
    Categories,
    Verdicts,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("search query is required")]
    EmptyQuery,
    #[error(
        "category slug may only contain lowercase ascii letters, digits, and hyphens"
    )]
    InvalidCategory,
}

#[must_use]
pub const fn trending_url() -> &'static str {
    "https://clawhub.ai/api/v1/trending?kind=skills"
}

pub fn category_url(slug: &str) -> Result<String, PlanError> {
    let slug = validate_category(slug)?;
    Ok(format!(
        "{ORIGIN}/api/v1/skills?category={slug}&sort=downloads&limit={PAGE_LIMIT_CATEGORY}&nonSuspiciousOnly=true"
    ))
}

pub fn search_url(
    query: &str,
    category: Option<&str>,
) -> Result<String, PlanError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(PlanError::EmptyQuery);
    }
    let mut url = format!(
        "{ORIGIN}/api/v1/search?q={}&limit={PAGE_LIMIT_SEARCH}&nonSuspiciousOnly=true",
        percent_encode(query)
    );
    if let Some(category) = category {
        url.push_str("&category=");
        url.push_str(validate_category(category)?);
    }
    Ok(url)
}

fn validate_category(slug: &str) -> Result<&str, PlanError> {
    let valid = !slug.is_empty()
        && slug.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
        });
    if valid {
        Ok(slug)
    } else {
        Err(PlanError::InvalidCategory)
    }
}

pub fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
        {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut encoded, "%{byte:02X}")
                .expect("writing to String cannot fail");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_url_is_one_page_of_downloads() {
        let url = category_url("knowledge").unwrap();
        assert_eq!(
            url,
            "https://clawhub.ai/api/v1/skills?category=knowledge&sort=downloads&limit=20&nonSuspiciousOnly=true"
        );
        assert!(category_url("nope/slash").is_err());
        assert!(!url.contains("cursor"));
    }

    #[test]
    fn search_url_keeps_category_when_present() {
        let url = search_url("pdf form", Some("knowledge")).unwrap();
        assert!(url.contains("q=pdf%20form") || url.contains("q=pdf+form"));
        assert!(url.contains("limit=15"));
        assert!(url.contains("nonSuspiciousOnly=true"));
        assert!(url.contains("category=knowledge"));
        assert!(search_url("   ", None).is_err());
    }

    #[test]
    fn search_encodes_non_ascii_and_rejects_bad_categories() {
        let url = search_url("图表 & pdf", Some("data-viz")).unwrap();
        assert!(url.contains("q=%E5%9B%BE%E8%A1%A8%20%26%20pdf"));
        assert_eq!(
            search_url("pdf", Some("../x")),
            Err(PlanError::InvalidCategory)
        );
    }
}
