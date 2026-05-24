//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_defs::definitions_for;
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{
    AgentTurnContext, ChatMessage, ExecutedToolCall, ToolCall, ToolName, ToolResult,
};
use crate::ports::outbound::{LlmProvider, ToolExecutionContext, ToolRegistry, ToolsConfig};
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
        context: AgentTurnContext,
    ) -> ClientResponse {
        if messages.is_empty() {
            return ClientResponse::error(
                id,
                ErrorCode::InvalidRequest,
                "messages must not be empty",
            );
        }

        let conversation = inject_shell_log_tail(messages, &context);

        if tools.is_empty() {
            return self.finish_text_only(id, &conversation).await;
        }

        if let Err(e) = context.validate_tools_enabled(&tools) {
            return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
        }

        let tool_ctx = ToolExecutionContext::new(
            context
                .client_cwd
                .clone()
                .expect("validate_tools_enabled ensures cwd"),
        );

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

/// `[shell log tail]` 前置（aibe application 内の唯一の注入箇所）。
fn inject_shell_log_tail(
    mut messages: Vec<ChatMessage>,
    context: &AgentTurnContext,
) -> Vec<ChatMessage> {
    if let Some(ref tail) = context.shell_log_tail {
        messages.insert(
            0,
            ChatMessage::user(format!("[shell log tail]\n{}", tail.as_str())),
        );
    }
    messages
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClientCwd, ShellLogTail};

    #[test]
    fn inject_shell_log_tail_skips_empty_normalized_tail() {
        let ctx = AgentTurnContext::for_text_only(None);
        let msgs = inject_shell_log_tail(vec![ChatMessage::user("hi")], &ctx);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hi");
    }

    #[test]
    fn inject_shell_log_tail_prepends_when_present() {
        let tail = ShellLogTail::from_wire_opt("log line").expect("tail");
        let ctx = AgentTurnContext::for_text_only(Some(tail));
        let msgs = inject_shell_log_tail(vec![ChatMessage::user("hi")], &ctx);
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].content.starts_with("[shell log tail]\n"));
        assert!(msgs[0].content.contains("log line"));
    }

    #[test]
    fn validate_tools_enabled_rejects_missing_cwd() {
        let ctx = AgentTurnContext::for_text_only(None);
        assert!(ctx
            .validate_tools_enabled(&[ToolName::read_file()])
            .is_err());
    }

    #[test]
    fn validate_tools_enabled_accepts_absolute_cwd() {
        let cwd = ClientCwd::parse("/tmp/proj").expect("cwd");
        let ctx = AgentTurnContext::for_tool_turn(cwd, None);
        assert!(ctx.validate_tools_enabled(&[ToolName::read_file()]).is_ok());
    }
}
