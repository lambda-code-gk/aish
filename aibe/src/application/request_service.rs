//! リクエスト種別のディスパッチ。

use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::domain::{parse_tool_names, ChatMessage};
use crate::ports::outbound::{LlmProvider, ToolRegistry, ToolsConfig};
use crate::protocol::{ClientRequest, ClientResponse, ErrorCode};

pub struct RequestService {
    agent_turn: AgentTurnService,
}

impl RequestService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        registry: Arc<dyn ToolRegistry>,
        tools_config: ToolsConfig,
    ) -> Self {
        Self {
            agent_turn: AgentTurnService::new(llm, registry, tools_config),
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

                // tools 非空: cwd を tool 名検証より先（0003 受け入れ条件 2）
                if !tools.is_empty() {
                    if let Err(e) = context.require_client_cwd() {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                }

                let tools = match parse_tool_names(tools) {
                    Ok(names) => names,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::ToolNotAllowed, e.to_string());
                    }
                };
                self.agent_turn.run(id, messages, tools, context).await
            }
        }
    }
}
