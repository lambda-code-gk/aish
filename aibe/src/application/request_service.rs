//! リクエスト種別のディスパッチ。

use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::domain::ChatMessage;
use crate::ports::outbound::LlmProvider;
use crate::protocol::{ClientRequest, ClientResponse};

pub struct RequestService {
    agent_turn: AgentTurnService,
}

impl RequestService {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self {
            agent_turn: AgentTurnService::new(llm),
        }
    }

    pub async fn handle(&self, request: ClientRequest) -> ClientResponse {
        match request {
            ClientRequest::Ping { id } => ClientResponse::Pong { id },
            ClientRequest::AgentTurn {
                id,
                messages,
                tools,
                context,
            } => {
                let messages: Vec<ChatMessage> = messages.into_iter().map(Into::into).collect();
                self.agent_turn.run(id, messages, tools, context).await
            }
        }
    }
}
