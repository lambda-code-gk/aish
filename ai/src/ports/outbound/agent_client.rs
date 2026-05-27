//! aibe への outbound port。

use aibe_protocol::ClientResponse;

use crate::domain::AskRequest;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("agent request failed: {0}")]
    Request(String),
    #[error("agent returned error: {code} — {message}")]
    Response { code: String, message: String },
}

/// 1 ターンのエージェント呼び出し。
pub trait AgentClient {
    fn agent_turn(&self, request: &AskRequest) -> Result<ClientResponse, AgentError>;
}
