//! Official website SSO URL builder (FlowyClaw client `?token=&language=` contract).

use nomi_config::DEFAULT_FLOWY_WEBSITE_URL;
use url::Url;

/// Where the official website should land after consuming the SSO token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WebsiteLanding {
    #[default]
    Home,
    Credits,
}

impl WebsiteLanding {
    /// Parse `landing` from `GET /api/cloud/website-entry`.
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim).filter(|item| !item.is_empty()) {
            Some(value) if value.eq_ignore_ascii_case("credits") => Self::Credits,
            _ => Self::Home,
        }
    }
}

/// Map UI locale to the website `language` query value.
pub fn website_language(language: &str) -> &'static str {
    if language.trim().to_ascii_lowercase().starts_with("zh") {
        "zh"
    } else {
        "en"
    }
}

/// Build `{website}/?token=…&language=zh` (token omitted when absent).
///
/// `WebsiteLanding::Credits` also sets `tab=credits` and `#pricing` so FlowyClaw
/// opens the homepage credits pack tab.
pub fn build_website_entry_url(
    website_url: &str,
    auth_token: Option<&str>,
    language: &str,
    landing: WebsiteLanding,
) -> String {
    let mut url = parse_website_base(website_url);
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(token) = auth_token.map(str::trim).filter(|value| !value.is_empty()) {
            pairs.append_pair("token", token);
        }
        pairs.append_pair("language", website_language(language));
        if landing == WebsiteLanding::Credits {
            pairs.append_pair("tab", "credits");
        }
    }
    if landing == WebsiteLanding::Credits {
        url.set_fragment(Some("pricing"));
    }
    url.to_string()
}

fn parse_website_base(website_url: &str) -> Url {
    let trimmed = website_url.trim();
    Url::parse(trimmed).unwrap_or_else(|_| {
        Url::parse(DEFAULT_FLOWY_WEBSITE_URL).expect("default website url is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomi_config::{DEFAULT_FLOWY_WEBSITE_URL, FLOWY_WEBSITE_HOST};

    fn parsed(url: &str) -> Url {
        Url::parse(url).unwrap()
    }

    fn query(url: &Url) -> Vec<(String, String)> {
        url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    #[test]
    fn language_maps_zh_prefix_and_falls_back_to_en() {
        assert_eq!(website_language("zh-CN"), "zh");
        assert_eq!(website_language("zh"), "zh");
        assert_eq!(website_language("en-US"), "en");
        assert_eq!(website_language("ja"), "en");
    }

    #[test]
    fn landing_from_query_only_accepts_credits() {
        assert_eq!(WebsiteLanding::from_query(None), WebsiteLanding::Home);
        assert_eq!(WebsiteLanding::from_query(Some("")), WebsiteLanding::Home);
        assert_eq!(WebsiteLanding::from_query(Some("home")), WebsiteLanding::Home);
        assert_eq!(
            WebsiteLanding::from_query(Some("credits")),
            WebsiteLanding::Credits
        );
        assert_eq!(
            WebsiteLanding::from_query(Some("CREDITS")),
            WebsiteLanding::Credits
        );
    }

    #[test]
    fn builds_sso_url_with_token_and_language() {
        let url = build_website_entry_url(
            DEFAULT_FLOWY_WEBSITE_URL,
            Some("jwt-token"),
            "zh-CN",
            WebsiteLanding::Home,
        );
        let parsed = parsed(&url);
        assert_eq!(parsed.host_str(), Some(FLOWY_WEBSITE_HOST));
        let query = query(&parsed);
        assert!(query.contains(&("token".into(), "jwt-token".into())));
        assert!(query.contains(&("language".into(), "zh".into())));
        assert!(!query.iter().any(|(k, _)| k == "tab"));
        assert_eq!(parsed.fragment(), None);
    }

    #[test]
    fn credits_landing_adds_tab_and_pricing_hash() {
        let url = build_website_entry_url(
            DEFAULT_FLOWY_WEBSITE_URL,
            Some("jwt-token"),
            "zh-CN",
            WebsiteLanding::Credits,
        );
        let parsed = parsed(&url);
        let query = query(&parsed);
        assert!(query.contains(&("token".into(), "jwt-token".into())));
        assert!(query.contains(&("language".into(), "zh".into())));
        assert!(query.contains(&("tab".into(), "credits".into())));
        assert_eq!(parsed.fragment(), Some("pricing"));
    }

    #[test]
    fn omits_token_when_logged_out() {
        let url = build_website_entry_url(
            DEFAULT_FLOWY_WEBSITE_URL,
            None,
            "en-US",
            WebsiteLanding::Home,
        );
        let parsed = parsed(&url);
        let keys: Vec<String> = parsed.query_pairs().map(|(k, _)| k.into_owned()).collect();
        assert!(!keys.contains(&"token".into()));
        assert_eq!(
            parsed
                .query_pairs()
                .find(|(k, _)| k == "language")
                .map(|(_, v)| v.into_owned()),
            Some("en".into())
        );
    }

    #[test]
    fn credits_landing_omits_token_when_logged_out() {
        let url = build_website_entry_url(
            DEFAULT_FLOWY_WEBSITE_URL,
            None,
            "zh",
            WebsiteLanding::Credits,
        );
        let parsed = parsed(&url);
        let keys: Vec<String> = parsed.query_pairs().map(|(k, _)| k.into_owned()).collect();
        assert!(!keys.contains(&"token".into()));
        assert!(keys.contains(&"tab".into()));
        assert_eq!(parsed.fragment(), Some("pricing"));
    }

    #[test]
    fn invalid_base_falls_back_to_default_host() {
        let url = build_website_entry_url("not a url", Some("abc"), "zh", WebsiteLanding::Home);
        let parsed = parsed(&url);
        assert_eq!(parsed.host_str(), Some(FLOWY_WEBSITE_HOST));
    }
}
