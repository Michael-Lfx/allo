use thiserror::Error;

pub const CATEGORY_TTL_SECS: u64 = 86_400;
pub const SEARCH_TTL_SECS: u64 = 900;
/// The All tab is the trending catalog, not a short highlight.
/// One request stays inside a single verdict batch.
pub const PAGE_LIMIT_TRENDING: u32 = 48;
/// One category request. Large enough to fill the skills grid,
/// small enough for a single verdict batch.
pub const PAGE_LIMIT_CATEGORY: u32 = 24;
/// Search has no cursor, so one response has to carry the whole shelf.
pub const PAGE_LIMIT_SEARCH: u32 = 48;
pub const TRENDING_CARD_CAP: usize = 240;
pub const CATEGORY_CARD_CAP: usize = 120;
pub const SEARCH_CARD_CAP: usize = 48;
pub const MAX_PACKAGE_BYTES: u64 = 8_388_608;

const ORIGIN: &str = "https://clawhub.ai";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetch {
    Trending {
        cursor: Option<String>,
    },
    Category {
        slug: String,
        cursor: Option<String>,
    },
    Search {
        query: String,
        category: Option<String>,
        cursor: Option<String>,
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
pub fn trending_url(cursor: Option<&str>) -> String {
    let mut url = format!(
        "{ORIGIN}/api/v1/trending?kind=skills&limit={PAGE_LIMIT_TRENDING}"
    );
    append_cursor(&mut url, cursor);
    url
}

pub fn category_url(
    slug: &str,
    cursor: Option<&str>,
) -> Result<String, PlanError> {
    let slug = validate_category(slug)?;
    let mut url = format!(
        "{ORIGIN}/api/v1/skills?category={slug}&sort=downloads&limit={PAGE_LIMIT_CATEGORY}&nonSuspiciousOnly=true"
    );
    append_cursor(&mut url, cursor);
    Ok(url)
}

pub fn search_url(
    query: &str,
    category: Option<&str>,
    cursor: Option<&str>,
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
    append_cursor(&mut url, cursor);
    Ok(url)
}

fn append_cursor(url: &mut String, cursor: Option<&str>) {
    let Some(cursor) =
        cursor.map(str::trim).filter(|cursor| !cursor.is_empty())
    else {
        return;
    };
    url.push_str("&cursor=");
    url.push_str(&percent_encode(cursor));
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
    fn category_url_is_one_page_until_a_cursor_is_supplied() {
        let url = category_url("knowledge", None).unwrap();
        assert_eq!(
            url,
            "https://clawhub.ai/api/v1/skills?category=knowledge&sort=downloads&limit=24&nonSuspiciousOnly=true"
        );
        assert!(category_url("nope/slash", None).is_err());
        assert!(!url.contains("cursor"));
        let continued = category_url("knowledge", Some("page two")).unwrap();
        assert!(continued.contains("cursor=page%20two"));
        assert!(continued.contains("limit=24"));
    }

    #[test]
    fn trending_url_limits_the_first_page_and_appends_a_cursor() {
        let url = trending_url(None);
        assert!(url.contains("kind=skills"));
        assert!(url.contains("limit=48"));
        assert!(!url.contains("cursor"));
        assert!(trending_url(Some("next")).contains("cursor=next"));
    }

    #[test]
    fn search_url_keeps_category_when_present() {
        let url = search_url("pdf form", Some("knowledge"), None).unwrap();
        assert!(url.contains("q=pdf%20form") || url.contains("q=pdf+form"));
        assert!(url.contains("limit=48"));
        assert!(url.contains("nonSuspiciousOnly=true"));
        assert!(url.contains("category=knowledge"));
        assert!(!url.contains("cursor"));
        assert!(search_url("   ", None, None).is_err());
    }

    #[test]
    fn search_encodes_non_ascii_and_rejects_bad_categories() {
        let url = search_url("图表 & pdf", Some("data-viz"), None).unwrap();
        assert!(url.contains("q=%E5%9B%BE%E8%A1%A8%20%26%20pdf"));
        assert_eq!(
            search_url("pdf", Some("../x"), None),
            Err(PlanError::InvalidCategory)
        );
    }
}
