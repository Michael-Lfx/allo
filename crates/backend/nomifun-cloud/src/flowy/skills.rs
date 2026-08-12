//! Flowy ViMax Skill Hub API client (`/vimax/skills/*`).

use nomifun_api_types::{
    VimaxCloudSkill, VimaxCloudSkillInstallResponse, VimaxCloudSkillLikeResponse,
    VimaxCloudSkillListResponse, VimaxCloudSkillPublishRequest, VimaxCloudSkillPublishResponse,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, FlowyApiClient};

impl FlowyApiClient {
    pub async fn vimax_skill_publish(
        &self,
        session: &ServerSession,
        body: &VimaxCloudSkillPublishRequest,
    ) -> Result<VimaxCloudSkillPublishResponse, ServerClientError> {
        self.post_data("/vimax/skills/publish", Some(session), body)
            .await
    }

    pub async fn vimax_skill_list(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        keyword: Option<&str>,
        category: Option<&str>,
        mode: Option<&str>,
        sort: Option<&str>,
        author_id: Option<i64>,
    ) -> Result<VimaxCloudSkillListResponse, ServerClientError> {
        let path = build_skill_list_path(
            "/vimax/skills/list",
            page,
            page_size,
            keyword,
            category,
            mode,
            sort,
            None,
            author_id,
        );
        self.get_data(&path, Some(session)).await
    }

    pub async fn vimax_skill_mine(
        &self,
        session: &ServerSession,
        page: Option<i32>,
        page_size: Option<i32>,
        status: Option<&str>,
    ) -> Result<VimaxCloudSkillListResponse, ServerClientError> {
        let path = build_skill_list_path(
            "/vimax/skills/mine",
            page,
            page_size,
            None,
            None,
            None,
            None,
            status,
            None,
        );
        self.get_data(&path, Some(session)).await
    }

    pub async fn vimax_skill_detail(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<VimaxCloudSkill, ServerClientError> {
        let path = format!("/vimax/skills/{id}");
        self.get_data(&path, Some(session)).await
    }

    pub async fn vimax_skill_install(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<VimaxCloudSkillInstallResponse, ServerClientError> {
        let path = format!("/vimax/skills/{id}/install");
        self.post_data(&path, Some(session), &serde_json::json!({}))
            .await
    }

    pub async fn vimax_skill_like(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<VimaxCloudSkillLikeResponse, ServerClientError> {
        let path = format!("/vimax/skills/{id}/like");
        self.post_data(&path, Some(session), &serde_json::json!({}))
            .await
    }

    pub async fn vimax_skill_unlike(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<VimaxCloudSkillLikeResponse, ServerClientError> {
        let path = format!("/vimax/skills/{id}/like");
        self.delete_data(&path, Some(session)).await
    }

    pub async fn vimax_skill_unpublish(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<(), ServerClientError> {
        let path = format!("/vimax/skills/{id}/unpublish");
        self.post_no_data(&path, Some(session), &serde_json::json!({}))
            .await
    }

    pub async fn vimax_skill_delete(
        &self,
        session: &ServerSession,
        id: i64,
    ) -> Result<(), ServerClientError> {
        let path = format!("/vimax/skills/{id}");
        self.delete_no_data(&path, Some(session)).await
    }
}

fn build_skill_list_path(
    base: &str,
    page: Option<i32>,
    page_size: Option<i32>,
    keyword: Option<&str>,
    category: Option<&str>,
    mode: Option<&str>,
    sort: Option<&str>,
    status: Option<&str>,
    author_id: Option<i64>,
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
    if let Some(m) = mode.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("mode".into(), m.to_string()));
    }
    if let Some(s) = sort.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("sort".into(), s.to_string()));
    }
    if let Some(st) = status.map(str::trim).filter(|s| !s.is_empty()) {
        pairs.push(("status".into(), st.to_string()));
    }
    if let Some(aid) = author_id.filter(|v| *v > 0) {
        pairs.push(("authorId".into(), aid.to_string()));
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
