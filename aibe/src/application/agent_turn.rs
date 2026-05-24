//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_defs::definitions_for;
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{ChatMessage, ExecutedToolCall, ToolCall, ToolName, ToolResult};
use crate::ports::outbound::{LlmProvider, ToolExecutionContext, ToolRegistry, ToolsConfig};
use crate::protocol::RequestContext;
use crate::protocol::{AgentTurnStatus, ClientResponse, ErrorCode, ProtocolMessageOut};

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    tools_config: ToolsConfig,
}

impl AgentTurnService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        registry: Arc<dyn ToolRegistry>,
        tools_config: ToolsConfig,
    ) -> Self {
        Self {
            llm,
            registry,
            tools_config,
        }
    }

    pub async fn run(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        context: RequestContext,
    ) -> ClientResponse {
        if messages.is_empty() {
            return ClientResponse::error(
                id,
                ErrorCode::InvalidRequest,
                "messages must not be empty",
            );
        }

        let mut conversation = messages;
        if let Some(ref tail) = context.shell_log_tail {
            if !tail.is_empty() {
                conversation.insert(0, ChatMessage::user(format!("[shell log tail]\n{tail}")));
            }
        }

        if tools.is_empty() {
            return self.finish_text_only(id, &conversation).await;
        }

        // tools 非空: cwd を tool 名より先に検証（0003 受け入れ条件 2）
        let tool_ctx = match tool_execution_context(&context) {
            Ok(ctx) => ctx,
            Err(msg) => {
                return ClientResponse::error(id, ErrorCode::InvalidRequest, msg);
            }
        };

        self.run_with_tools(id, conversation, tools, tool_ctx).await
    }

    async fn finish_text_only(&self, id: String, conversation: &[ChatMessage]) -> ClientResponse {
        match self.llm.complete(conversation).await {
            Ok(assistant) => ClientResponse::AgentTurnResult {
                id,
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut::from_assistant(&assistant),
                tool_calls: vec![],
            },
            Err(e) => client_response_for_llm_error(id, e),
        }
    }

    async fn run_with_tools(
        &self,
        id: String,
        mut conversation: Vec<ChatMessage>,
        allowed_tools: Vec<ToolName>,
        tool_ctx: ToolExecutionContext,
    ) -> ClientResponse {
        let tool_defs = definitions_for(&allowed_tools);
        let mut executed: Vec<ExecutedToolCall> = Vec::new();
        let max_rounds = self.tools_config.max_rounds.max(1);

        for round in 0..max_rounds {
            let step = match self
                .llm
                .complete_with_tools(&conversation, &tool_defs)
                .await
            {
                Ok(s) => s,
                Err(e) => return client_response_for_llm_error(id, e),
            };

            if step.tool_calls.is_empty() {
                return ClientResponse::AgentTurnResult {
                    id,
                    status: AgentTurnStatus::Ok,
                    assistant_message: ProtocolMessageOut::from_assistant(&step.assistant),
                    tool_calls: executed,
                };
            }

            conversation.push(step.assistant.clone());

            for tc in &step.tool_calls {
                let (record, result) = if !allowed_tools.contains(&tc.name) {
                    rejected_tool_result(
                        tc,
                        "tool_not_allowed",
                        format!("model requested disallowed tool: {}", tc.name),
                    )
                } else if let Some(executor) = self.registry.get(&tc.name) {
                    executor
                        .execute(
                            &tc.id,
                            &tc.arguments,
                            self.tools_config.exec_timeout_ms,
                            &tool_ctx,
                        )
                        .await
                } else {
                    rejected_tool_result(
                        tc,
                        "tool_not_implemented",
                        format!("tool not implemented: {}", tc.name),
                    )
                };
                executed.push(record);
                let content = if result.is_error {
                    format!("[tool error]\n{}", result.content)
                } else {
                    result.content
                };
                conversation.push(ChatMessage::tool(tc.id.clone(), content));
            }

            if round + 1 >= max_rounds {
                return finish_after_max_tool_rounds(
                    self.llm.as_ref(),
                    id,
                    &conversation,
                    executed,
                    max_rounds,
                )
                .await;
            }
        }

        ClientResponse::error(
            id,
            ErrorCode::InternalError,
            "agent loop ended unexpectedly",
        )
    }
}

/// モデルが許可外・未実装ツールを要求したときの tool result（ループ継続）。
fn rejected_tool_result(
    tc: &ToolCall,
    error: &str,
    message: String,
) -> (ExecutedToolCall, ToolResult) {
    let record = ExecutedToolCall::err(
        tc.id.clone(),
        tc.name.clone(),
        tc.arguments.clone(),
        error,
        message.clone(),
    );
    let result = ToolResult {
        tool_call_id: tc.id.clone(),
        content: message,
        is_error: true,
    };
    (record, result)
}

fn tool_execution_context(context: &RequestContext) -> Result<ToolExecutionContext, String> {
    context
        .require_client_cwd()
        .map(ToolExecutionContext::new)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_execution_context_requires_cwd() {
        let ctx = RequestContext::default();
        assert!(tool_execution_context(&ctx).is_err());
    }

    #[test]
    fn tool_execution_context_accepts_absolute_cwd() {
        let ctx = RequestContext {
            cwd: Some("/tmp/proj".into()),
            ..Default::default()
        };
        assert!(tool_execution_context(&ctx).is_ok());
    }
}
