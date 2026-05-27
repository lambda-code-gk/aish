//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{AgentTurnContext, ChatMessage, ToolName};
use crate::ports::outbound::{
    LlmProvider, TerminationCapability, ToolExecutionContext, ToolRoundTerminator,
};
use aibe_protocol::{AgentTurnStatus, ClientResponse, ErrorCode};

use crate::application::protocol_convert::protocol_message_out_from_chat;

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
    executor: ToolRoundExecutor,
    terminator: Arc<dyn ToolRoundTerminator>,
    termination_capability: TerminationCapability,
}

impl AgentTurnService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
    ) -> Self {
        Self {
            llm,
            executor,
            terminator,
            termination_capability,
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
                assistant_message: protocol_message_out_from_chat(&assistant),
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
        let mut executed = Vec::new();
        let max_rounds = self.executor.tools_config().effective_max_rounds();

        for round in 0..max_rounds {
            match self
                .executor
                .run_one_round(&conversation, &allowed_tools, &tool_ctx, &executed)
                .await
            {
                Ok(RoundOutcome::Completed {
                    assistant,
                    executed: round_executed,
                }) => {
                    return ClientResponse::AgentTurnResult {
                        id,
                        status: AgentTurnStatus::Ok,
                        assistant_message: protocol_message_out_from_chat(&assistant),
                        tool_calls: round_executed,
                    };
                }
                Ok(RoundOutcome::Continue {
                    conversation: next,
                    executed: round_executed,
                }) => {
                    conversation = next;
                    executed = round_executed;

                    if round + 1 >= max_rounds {
                        return finish_after_max_tool_rounds(
                            self.llm.as_ref(),
                            self.terminator.as_ref(),
                            &self.termination_capability,
                            id,
                            &conversation,
                            executed,
                            max_rounds,
                        )
                        .await;
                    }
                }
                Err(e) => return client_response_for_llm_error(id, e),
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
