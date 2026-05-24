//! リクエスト種別のディスパッチ。

use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{parse_tool_names, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError};
use crate::ports::outbound::{LlmProvider, TerminationCapability, ToolRoundTerminator};
use crate::protocol::{
    ClientRequest, ClientResponse, ErrorCode, ProtocolMessageConversionError, RequestContext,
};

pub struct RequestService {
    agent_turn: AgentTurnService,
}

impl RequestService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
    ) -> Self {
        Self {
            agent_turn: AgentTurnService::new(llm, executor, terminator, termination_capability),
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
                let messages: Vec<ChatMessage> = match messages
                    .into_iter()
                    .map(ChatMessage::try_from)
                    .collect::<Result<Vec<_>, ProtocolMessageConversionError>>()
                {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                };

                // tools 非空: cwd を tool 名検証より先（0003 / 0005 受け入れ条件）
                if !tools.is_empty() {
                    if let Err(e) = validate_client_cwd_for_tools(&context) {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                }

                let tools = match parse_tool_names(tools) {
                    Ok(names) => names,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::ToolNotAllowed, e.to_string());
                    }
                };

                let ctx = match AgentTurnContext::try_from(context) {
                    Ok(ctx) => ctx,
                    Err(e) => match e {},
                };

                self.agent_turn.run(id, messages, tools, ctx).await
            }
        }
    }
}

fn validate_client_cwd_for_tools(context: &RequestContext) -> Result<(), ClientCwdError> {
    match context.cwd.as_deref() {
        Some(raw) => ClientCwd::parse(raw).map(|_| ()),
        None => Err(ClientCwdError::Missing),
    }
}
