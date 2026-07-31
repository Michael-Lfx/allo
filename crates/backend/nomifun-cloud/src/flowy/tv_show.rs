//! Flowy ViMax TV Show API client (`/vimax/tv-show/*`).

use nomifun_api_types::{
    TvShowLikeResponse, TvShowListResponse, TvShowPublishRequest, TvShowPublishResponse,
    TvShowVideo,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, handle_http_and_envelope, FlowyApiClient};

impl FlowyApiClient {
    pub async fn tv_show_publish(
        &self,
        session: &ServerSession,
        body: &TvShowPublishRequest,
    ) -> Result<TvShowPublishResponse, ServerClientError> {
        self.post_data("/vimax/tv-show/publish", Some(session), body)
            .await
    }

    pub async fn tv_show_list(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        workflow: Option<&str>,
        keyword: Option<&str>,
        sort: Option<&str>,
    ) -> Result<TvShowListResponse, ServerClientError> {
        let path = build_tv_show_list_path("/vimax/tv-show/list", page, page_size, workflow, keyword, sort, None);
        self.get_data(&path, Some(session)).await
    }

    pub async fn tv_show_mine(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        status: Option<&str>,
    ) -> Result<TvShowListResponse, ServerClientError> {
        let path = build_tv_show_list_path(
            "/vimax/tv-show/mine",
            page,
            page_size,
            None,
            None,
            None,
            status,
        );
        self.get_data(&path, Some(session)).await
    }

    pub async fn tv_show_detail(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<TvShowVideo, ServerClientError> {
        let path = format!("/vimax/tv-show/{id}");
        self.get_data(&path, Some(session)).await
    }

    pub async fn tv_show_like(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<TvShowLikeResponse, ServerClientError> {
        let path = format!("/vimax/tv-show/{id}/like");
        self.post_data(&path, Some(session), &serde_json::json!({}))
            .await
    }

    pub async fn tv_show_unlike(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<TvShowLikeResponse, ServerClientError> {
        let path = format!("/vimax/tv-show/{id}/like");
        self.delete_data(&path, Some(session)).await
    }

    pub async fn tv_show_delete(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<(), ServerClientError> {
        let path = format!("/vimax/tv-show/{id}");
        self.delete_no_data(&path, Some(session)).await
    }

    async fn delete_data<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        session: Option<&ServerSession>,
    ) -> Result<T, ServerClientError> {
        let env = self.delete_envelope(path, session).await?;
        env.into_data()
    }

    async fn delete_no_data(
        &self,
        path: &str,
        session: Option<&ServerSession>,
    ) -> Result<(), ServerClientError> {
        let env = self.delete_envelope(path, session).await?;
        env.ensure_ok_no_data()
    }

    async fn delete_envelope(
        &self,
        path: &str,
        session: Option<&ServerSession>,
    ) -> Result<super::FlowyEnvelope, ServerClientError> {
        let resp = self.transport.delete(path, session).await?;
        let status = resp.status().as_u16();
        let body = resp
            .text()
            .await
            .map_err(|e| ServerClientError::Http(e.to_string()))?;
        handle_http_and_envelope(status, &body)
    }
}

fn build_tv_show_list_path(
    base: &str,
    page: Option<i32>,
    page_size: Option<i32>,
    workflow: Option<&str>,
    keyword: Option<&str>,
    sort: Option<&str>,
    status: Option<&str>,
) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(p) = page.filter(|v| *v > 0) {
        pairs.push(("page".into(), p.to_string()));
    }
    if let Some(ps) = page_size.filter(|v| *v > 0) {
        pairs.push(("pageSize".into(), ps.to_string()));
    }
    if let Some(w) = workflow.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("workflow".into(), w.to_string()));
    }
    if let Some(k) = keyword.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("keyword".into(), k.to_string()));
    }
    if let Some(s) = sort.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("sort".into(), s.to_string()));
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
