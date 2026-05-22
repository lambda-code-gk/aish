//! `agent_turn` ユースケース。

use std::sync::Arc;

use crate::domain::ChatMessage;
use crate::ports::outbound::{LlmError, LlmProvider};
use crate::protocol::RequestContext;
use crate::protocol::{ClientResponse, ErrorCode, ProtocolMessageOut};

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
}

impl AgentTurnService {
    pub fn new(llm: Arc<dyn LlmProvider>) -> Self {
        Self { llm }
    }

    pub async fn run(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        _tools: Vec<String>,
        context: RequestContext,
    ) -> ClientResponse {
        if messages.is_empty() {
            return ClientResponse::error(
                id,
                ErrorCode::InvalidRequest,
                "messages must not be empty",
            );
        }

        let mut prompt_messages = messages;
        if let Some(tail) = context.shell_log_tail {
            if !tail.is_empty() {
                prompt_messages.insert(0, ChatMessage::user(format!("[shell log tail]\n{tail}")));
            }
        }

        match self.llm.complete(&prompt_messages).await {
            Ok(assistant) => ClientResponse::AgentTurnResult {
                id,
                status: "ok".to_string(),
                assistant_message: ProtocolMessageOut::from(assistant),
                tool_calls: vec![],
            },
            Err(LlmError::Provider(msg)) => {
                ClientResponse::error(id, ErrorCode::ProviderError, msg)
            }
        }
    }
}
