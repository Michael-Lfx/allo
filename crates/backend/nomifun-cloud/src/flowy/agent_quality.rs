use nomifun_api_types::{
    AgentQualityAck, AgentQualityBadcaseRequest, AgentQualityPromotedItem, AgentQualityRunRequest,
};

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::FlowyApiClient;

impl FlowyApiClient {
    pub async fn submit_agent_badcase(
        &self,
        session: &ServerSession,
        request: &AgentQualityBadcaseRequest,
    ) -> Result<AgentQualityAck, ServerClientError> {
        self.post_data("/agent-quality/badcases", Some(session), request)
            .await
    }

    pub async fn submit_agent_eval_run(
        &self,
        session: &ServerSession,
        request: &AgentQualityRunRequest,
    ) -> Result<AgentQualityAck, ServerClientError> {
        self.post_data("/agent-quality/runs", Some(session), request)
            .await
    }

    pub async fn list_promoted_agent_badcases(
        &self,
        session: &ServerSession,
    ) -> Result<Vec<AgentQualityPromotedItem>, ServerClientError> {
        self.get_data("/agent-quality/badcases/promoted", Some(session))
            .await
    }
}
