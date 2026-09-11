//! Flowy canvas Generation Template API (`/generation-templates/*`).

use nomifun_api_types::{
    GenerationTemplateDetail, GenerationTemplateEventRequest, GenerationTemplateListResponse,
    GenerationTemplatePublishRequest,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, FlowyApiClient};

impl FlowyApiClient {
    pub async fn generation_template_list(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        keyword: Option<&str>,
        category: Option<&str>,
        origin: Option<&str>,
        sort: Option<&str>,
        node_type: Option<&str>,
    ) -> Result<GenerationTemplateListResponse, ServerClientError> {
        let path = build_generation_template_list_path(
            "/generation-templates",
            page,
            page_size,
            keyword,
            category,
            origin,
            sort,
            node_type,
            None,
        );
        self.get_data(&path, Some(session)).await
    }

    pub async fn generation_template_mine(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        status: Option<&str>,
    ) -> Result<GenerationTemplateListResponse, ServerClientError> {
        let path = build_generation_template_list_path(
            "/generation-templates/mine",
            page,
            page_size,
            None,
            None,
            None,
            None,
            None,
            status,
        );
        self.get_data(&path, Some(session)).await
    }

    pub async fn generation_template_detail(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<GenerationTemplateDetail, ServerClientError> {
        let path = format!("/generation-templates/{id}");
        self.get_data(&path, Some(session)).await
    }

    pub async fn generation_template_event(
        &self,
        session: &ServerSession,
        id: i64,
        event_type: &str,
    ) -> Result<(), ServerClientError> {
        let path = format!("/generation-templates/{id}/events");
        let body = GenerationTemplateEventRequest {
            event_type: event_type.to_string(),
        };
        self.post_no_data(&path, Some(session), &body).await
    }

    pub async fn generation_template_publish_from_canvas(
        &self,
        session: &ServerSession,
        body: &GenerationTemplatePublishRequest,
    ) -> Result<GenerationTemplateDetail, ServerClientError> {
        self.post_data(
            "/generation-templates/publish-from-canvas",
            Some(session),
            body,
        )
        .await
    }
}

fn build_generation_template_list_path(
    base: &str,
    page: Option<i32>,
    page_size: Option<i32>,
    keyword: Option<&str>,
    category: Option<&str>,
    origin: Option<&str>,
    sort: Option<&str>,
    node_type: Option<&str>,
    status: Option<&str>,
) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(p) = page.filter(|v| *v > 0) {
        pairs.push(("page".into(), p.to_string()));
    }
    if let Some(ps) = page_size.filter(|v| *v > 0) {
        pairs.push(("pageSize".into(), ps.to_string()));
    }
    if let Some(k) = keyword.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("keyword".into(), k.to_string()));
    }
    if let Some(c) = category.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("category".into(), c.to_string()));
    }
    if let Some(o) = origin.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("origin".into(), o.to_string()));
    }
    if let Some(s) = sort.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("sort".into(), s.to_string()));
    }
    if let Some(nt) = node_type.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("nodeType".into(), nt.to_string()));
    }
    if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("status".into(), st.to_string()));
    }
    if pairs.is_empty() {
        return base.to_string();
    }
    let query = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", form_urlencode(&v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}
