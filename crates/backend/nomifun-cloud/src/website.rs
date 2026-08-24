//! Official website SSO URL builder (FlowyClaw client `?token=&language=` contract).

use nomi_config::DEFAULT_FLOWY_WEBSITE_URL;
use url::Url;

/// Map UI locale to the website `language` query value.
pub fn website_language(language: &str) -> &'static str {
    if language.trim().to_ascii_lowercase().starts_with("zh") {
        "zh"
    } else {
        "en"
    }
}

/// Build `{website}/?token=…&language=zh` (token omitted when absent).
pub fn build_website_entry_url(
    website_url: &str,
    auth_token: Option<&str>,
    language: &str,
) -> String {
    let mut url = parse_website_base(website_url);
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(token) = auth_token.map(str::trim).filter(|value| !value.is_empty()) {
            pairs.append_pair("token", token);
        }
        pairs.append_pair("language", website_language(language));
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

    #[test]
    fn language_maps_zh_prefix_and_falls_back_to_en() {
        assert_eq!(website_language("zh-CN"), "zh");
        assert_eq!(website_language("zh"), "zh");
        assert_eq!(website_language("en-US"), "en");
        assert_eq!(website_language("ja"), "en");
    }

    #[test]
    fn builds_sso_url_with_token_and_language() {
        let url = build_website_entry_url(DEFAULT_FLOWY_WEBSITE_URL, Some("jwt-token"), "zh-CN");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some(FLOWY_WEBSITE_HOST));
        let query: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert!(query.contains(&("token".into(), "jwt-token".into())));
        assert!(query.contains(&("language".into(), "zh".into())));
    }

    #[test]
    fn omits_token_when_logged_out() {
        let url = build_website_entry_url(DEFAULT_FLOWY_WEBSITE_URL, None, "en-US");
        let parsed = Url::parse(&url).unwrap();
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
    fn invalid_base_falls_back_to_default_host() {
        let url = build_website_entry_url("not a url", Some("abc"), "zh");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some(FLOWY_WEBSITE_HOST));
    }
}
