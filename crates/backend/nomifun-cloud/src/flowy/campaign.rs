//! Flowy ViMax campaign marketing API client (`/vimax/campaigns/*`).

use nomifun_api_types::{
    CampaignCarouselResponse, CampaignDetail, CampaignListResponse, TvShowListResponse,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, FlowyApiClient};

impl FlowyApiClient {
    pub async fn campaign_carousel(
        &self,
        session: &ServerSession,
    ) -> Result<CampaignCarouselResponse, ServerClientError> {
        self.get_data("/vimax/campaigns/carousel", Some(session))
            .await
    }

    pub async fn campaign_list(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        include_ended: Option<bool>,
    ) -> Result<CampaignListResponse, ServerClientError> {
        let path = build_campaign_list_path(page, page_size, include_ended);
        self.get_data(&path, Some(session)).await
    }

    pub async fn campaign_detail(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<CampaignDetail, ServerClientError> {
        let path = format!("/vimax/campaigns/{id}");
        self.get_data(&path, Some(session)).await
    }

    pub async fn campaign_submissions(
        &self,
        session: &ServerSession,
        id: i64,
        page: Option<i32>,
        page_size: Option<i32>,
        workflow: Option<&str>,
        keyword: Option<&str>,
        sort: Option<&str>,
    ) -> Result<TvShowListResponse, ServerClientError> {
        let path = build_campaign_submissions_path(id, page, page_size, workflow, keyword, sort);
        self.get_data(&path, Some(session)).await
    }

    pub async fn campaign_winners(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<TvShowListResponse, ServerClientError> {
        let path = format!("/vimax/campaigns/{id}/winners");
        self.get_data(&path, Some(session)).await
    }
}

fn build_campaign_list_path(
    page: Option<i32>,
    page_size: Option<i32>,
    include_ended: Option<bool>,
) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(p) = page.filter(|v| *v > 0) {
        pairs.push(("page".into(), p.to_string()));
    }
    if let Some(ps) = page_size.filter(|v| *v > 0) {
        pairs.push(("pageSize".into(), ps.to_string()));
    }
    if let Some(true) = include_ended {
        pairs.push(("includeEnded".into(), "true".into()));
    }
    if pairs.is_empty() {
        return "/vimax/campaigns/list".into();
    }
    let query = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", form_urlencode(&v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("/vimax/campaigns/list?{query}")
}

fn build_campaign_submissions_path(
    id: i64,
    page: Option<i32>,
    page_size: Option<i32>,
    workflow: Option<&str>,
    keyword: Option<&str>,
    sort: Option<&str>,
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
    let base = format!("/vimax/campaigns/{id}/submissions");
    if pairs.is_empty() {
        return base;
    }
    let query = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", form_urlencode(&v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}
