//! Flowy expert catalog on the claw host: `GET /experts` and `GET /experts/:slug`.
//!
//! The catalog spec is written as `/api/v1/experts`. That path is not mounted
//! under `https://server.<domain>/claw` (it returns HTTP 200 with an empty
//! body). The live route is `/experts`.
//!
//! Each summon still records one download with
//! `POST /api/v1/experts/:id/download`. `:id` is the list row id, the body is
//! empty, and the call carries the cloud `Authorization: Bearer` token.

use nomifun_api_types::{ExpertDetail, ExpertListResponse};

use crate::error::ServerClientError;
use crate::flowy::response::{handle_http_and_envelope, FlowyEnvelope};
use crate::session::ServerSession;

use super::{form_urlencode, FlowyApiClient};

const EXPERTS_PATH: &str = "/experts";
const EXPERT_DOWNLOAD_PATH: &str = "/api/v1/experts";
const MAX_PAGE_SIZE: i32 = 100;
const MAX_KEYWORD_CHARS: usize = 200;
const MAX_CATEGORY_CHARS: usize = 128;
const MAX_SLUG_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpertListParams {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub keyword: Option<String>,
    pub expert_type: Option<String>,
    pub category_id: Option<String>,
}

pub(crate) fn prepare_expert_list_query(
    page: Option<i32>,
    page_size: Option<i32>,
    keyword: Option<String>,
    expert_type: Option<String>,
    category_id: Option<String>,
) -> Result<ExpertListParams, &'static str> {
    Ok(ExpertListParams {
        page: normalize_page(page)?,
        page_size: normalize_page_size(page_size)?,
        keyword: trim_limit(keyword, MAX_KEYWORD_CHARS),
        expert_type: normalize_expert_type(expert_type)?,
        category_id: normalize_category_id(category_id)?,
    })
}

pub fn prepare_expert_slug(slug: &str) -> Result<String, &'static str> {
    let slug = slug.trim();
    if slug.is_empty() || slug.chars().count() > MAX_SLUG_CHARS {
        return Err("invalid expert slug");
    }
    if slug.starts_with('.') || slug.contains("..") {
        return Err("invalid expert slug");
    }
    if !slug
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("invalid expert slug");
    }
    Ok(slug.to_string())
}

pub(crate) fn is_expert_not_found(err: &ServerClientError) -> bool {
    match err {
        ServerClientError::Api { code, msg } => *code == 404 || msg.contains("expert_not_found"),
        ServerClientError::Server { status, body, .. } => {
            *status == 404 || body.contains("expert_not_found")
        }
        ServerClientError::InvalidResponse(msg) => msg.contains("expert_not_found"),
        _ => false,
    }
}

pub(crate) fn expert_list_path(params: &ExpertListParams) -> String {
    let mut pairs: Vec<(&str, String)> = Vec::new();
    if let Some(page) = params.page {
        pairs.push(("page", page.to_string()));
    }
    if let Some(page_size) = params.page_size {
        pairs.push(("pageSize", page_size.to_string()));
    }
    if let Some(keyword) = params.keyword.as_deref() {
        pairs.push(("keyword", keyword.to_string()));
    }
    if let Some(expert_type) = params.expert_type.as_deref() {
        pairs.push(("expertType", expert_type.to_string()));
    }
    if let Some(category_id) = params.category_id.as_deref() {
        pairs.push(("categoryId", category_id.to_string()));
    }
    if pairs.is_empty() {
        return EXPERTS_PATH.to_string();
    }
    let query = pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={}", form_urlencode(&value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{EXPERTS_PATH}?{query}")
}

pub(crate) fn expert_detail_path(slug: &str) -> String {
    format!("{EXPERTS_PATH}/{}", form_urlencode(slug))
}

pub(crate) fn prepare_expert_id(id: &str) -> Result<String, &'static str> {
    let id = id.trim();
    if id.is_empty() {
        return Err("expert id is missing");
    }
    if id.chars().count() > 128 || id.contains("..") {
        return Err("invalid expert id");
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err("invalid expert id");
    }
    Ok(id.to_string())
}

pub(crate) fn expert_download_path(id: &str) -> String {
    format!("{EXPERT_DOWNLOAD_PATH}/{}/download", form_urlencode(id))
}

pub(crate) fn interpret_download_count(status: u16, body: &str) -> Result<(), ServerClientError> {
    if status == 404 {
        return Err(not_found_error(body));
    }
    if (200..300).contains(&status) {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        return match FlowyEnvelope::parse_body(trimmed) {
            Ok(env) => env.ensure_ok_no_data(),
            Err(_) if trimmed.contains("error.expert_not_found") => Err(not_found_error(trimmed)),
            Err(_) => Ok(()),
        };
    }
    match handle_http_and_envelope(status, body) {
        Ok(env) => env.ensure_ok_no_data(),
        Err(error) => Err(error),
    }
}

fn not_found_error(body: &str) -> ServerClientError {
    let body = body.trim();
    ServerClientError::Server {
        status: 404,
        body: if body.is_empty() {
            "error.expert_not_found".into()
        } else {
            body.to_string()
        },
        request_id: None,
    }
}

fn normalize_page(page: Option<i32>) -> Result<Option<i32>, &'static str> {
    match page {
        None => Ok(None),
        Some(value) if value >= 1 => Ok(Some(value)),
        Some(_) => Err("page must be >= 1"),
    }
}

fn normalize_page_size(page_size: Option<i32>) -> Result<Option<i32>, &'static str> {
    match page_size {
        None => Ok(None),
        Some(value) if (1..=MAX_PAGE_SIZE).contains(&value) => Ok(Some(value)),
        Some(value) if value > MAX_PAGE_SIZE => Ok(Some(MAX_PAGE_SIZE)),
        Some(_) => Err("pageSize must be >= 1"),
    }
}

fn normalize_expert_type(value: Option<String>) -> Result<Option<String>, &'static str> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed == "agent" || trimmed == "team" {
        return Ok(Some(trimmed.to_string()));
    }
    Err("expertType must be agent or team")
}

fn normalize_category_id(value: Option<String>) -> Result<Option<String>, &'static str> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_CATEGORY_CHARS
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err("invalid categoryId");
    }
    Ok(Some(trimmed.to_string()))
}

fn trim_limit(value: Option<String>, max_chars: usize) -> Option<String> {
    let trimmed = value.unwrap_or_default();
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(max_chars).collect())
}

impl FlowyApiClient {
    pub(crate) async fn list_experts(
        &self,
        session: &ServerSession,
        params: &ExpertListParams,
    ) -> Result<ExpertListResponse, ServerClientError> {
        self.get_data(&expert_list_path(params), Some(session)).await
    }

    pub(crate) async fn get_expert(
        &self,
        session: &ServerSession,
        slug: &str,
    ) -> Result<ExpertDetail, ServerClientError> {
        self.get_data(&expert_detail_path(slug), Some(session)).await
    }

    /// `POST /api/v1/experts/:id/download` with no body. The session supplies
    /// `Authorization: Bearer`.
    pub(crate) async fn record_expert_download(
        &self,
        session: &ServerSession,
        id: &str,
    ) -> Result<(), ServerClientError> {
        let id = prepare_expert_id(id).map_err(|message| ServerClientError::Api {
            code: 400,
            msg: message.into(),
        })?;
        let response = self
            .transport
            .request(
                reqwest::Method::POST,
                &expert_download_path(&id),
                Some(session),
                None,
            )
            .await?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| ServerClientError::Http(error.to_string()))?;
        interpret_download_count(status, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_path_includes_catalog_filters() {
        let params = prepare_expert_list_query(
            Some(2),
            Some(250),
            Some("  前端 ".into()),
            Some("team".into()),
            Some("02-Engineering".into()),
        )
        .expect("query");
        assert_eq!(params.page_size, Some(100));
        assert_eq!(
            expert_list_path(&params),
            "/experts?page=2&pageSize=100&keyword=%E5%89%8D%E7%AB%AF&expertType=team&categoryId=02-Engineering"
        );
    }

    #[test]
    fn empty_filters_keep_the_bare_list_path() {
        let params = prepare_expert_list_query(None, None, Some("  ".into()), None, None)
            .expect("query");
        assert_eq!(expert_list_path(&params), "/experts");
    }

    #[test]
    fn rejects_unknown_expert_type_and_bad_page() {
        let err = prepare_expert_list_query(None, None, None, Some("plugin".into()), None)
            .expect_err("type");
        assert_eq!(err, "expertType must be agent or team");
        let err = prepare_expert_list_query(Some(0), None, None, None, None).expect_err("page");
        assert_eq!(err, "page must be >= 1");
    }

    #[test]
    fn slug_allows_zip_names_and_rejects_traversal() {
        assert_eq!(
            prepare_expert_slug(" stock-partner-team ").expect("slug"),
            "stock-partner-team"
        );
        assert!(prepare_expert_slug("../secret").is_err());
        assert!(prepare_expert_slug(".hidden").is_err());
        assert!(prepare_expert_slug("a/b").is_err());
    }

    #[test]
    fn detail_path_encodes_the_slug() {
        assert_eq!(
            expert_detail_path("demo-expert"),
            "/experts/demo-expert"
        );
    }

    #[test]
    fn download_path_uses_the_list_id() {
        assert_eq!(
            expert_download_path(&prepare_expert_id(" 8801 ").expect("id")),
            "/api/v1/experts/8801/download"
        );
        assert_eq!(prepare_expert_id(""), Err("expert id is missing"));
        assert!(prepare_expert_id("../secret").is_err());
        assert!(prepare_expert_id("a/b").is_err());
    }

    #[test]
    fn download_count_accepts_an_empty_success_and_rejects_not_found() {
        assert!(interpret_download_count(200, "").is_ok());
        assert!(interpret_download_count(204, "").is_ok());
        assert!(interpret_download_count(200, r#"{"code":200,"msg":"ok"}"#).is_ok());
        let missing = interpret_download_count(
            404,
            r#"{"error_key":"error.expert_not_found"}"#,
        )
        .expect_err("missing");
        assert!(is_expert_not_found(&missing));
        let unpublished = interpret_download_count(200, r#"{"code":404,"msg":"error.expert_not_found"}"#)
            .expect_err("unpublished");
        assert!(is_expert_not_found(&unpublished));
    }

    #[test]
    fn not_found_matches_the_catalog_error_code() {
        let err = ServerClientError::Api {
            code: 404,
            msg: "error.expert_not_found".into(),
        };
        assert!(is_expert_not_found(&err));
        let other = ServerClientError::Api {
            code: 500,
            msg: "unavailable".into(),
        };
        assert!(!is_expert_not_found(&other));
    }
}
