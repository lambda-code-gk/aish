//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{AgentTurnContext, ChatMessage, ToolName};
use crate::ports::outbound::{
    LlmProvider, ShellExecApprovalGate, TerminationCapability, ToolExecutionContext,
    ToolRoundTerminator, TurnCancellation, TurnEventSink,
};
use aibe_protocol::{AgentTurnStatus, ClientResponse, ErrorCode, ProgressPhase};

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
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    ) -> ClientResponse {
        self.run_with_events(id, messages, tools, context, approval_gate, None, None)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_events(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        context: AgentTurnContext,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
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
            return self
                .finish_text_only(
                    id,
                    &conversation,
                    events.as_ref().map(Arc::as_ref),
                    cancellation.as_ref().map(Arc::as_ref),
                )
                .await;
        }

        if let Err(e) = context.validate_tools_enabled(&tools) {
            return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
        }

        let mut tool_ctx = ToolExecutionContext::new(
            context
                .client_cwd
                .clone()
                .expect("validate_tools_enabled ensures cwd"),
        )
        .with_turn_id(id.clone());
        if let Some(gate) = approval_gate {
            tool_ctx = tool_ctx.with_approval_gate(gate);
        }

        self.run_with_tools(id, conversation, tools, tool_ctx, events, cancellation)
            .await
    }

    async fn finish_text_only(
        &self,
        id: String,
        conversation: &[ChatMessage],
        events: Option<&dyn TurnEventSink>,
        cancellation: Option<&TurnCancellation>,
    ) -> ClientResponse {
        if let Some(cancel) = cancellation {
            if cancel.is_cancelled() {
                if let Some(events) = events {
                    events
                        .progress(&id, ProgressPhase::Cancelling, Some("cancelled".into()))
                        .await;
                }
                return ClientResponse::Cancelled {
                    id: id.clone(),
                    turn_id: id,
                    reason: Some("cancelled".into()),
                };
            }
        }
        if let Some(events) = events {
            events
                .progress(
                    &id,
                    ProgressPhase::Thinking,
                    Some("generating response".into()),
                )
                .await;
        }
        let assistant = if let Some(cancel) = cancellation {
            tokio::select! {
                res = self.llm.complete(conversation) => res,
                _ = cancel.wait() => {
                    if let Some(events) = events {
                        events.progress(&id, ProgressPhase::Cancelling, Some("cancelled".into())).await;
                    }
                    return ClientResponse::Cancelled {
                        id: id.clone(),
                        turn_id: id,
                        reason: Some("cancelled".into()),
                    };
                }
            }
        } else {
            self.llm.complete(conversation).await
        };

        match assistant {
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
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        let mut executed = Vec::new();
        let max_rounds = self.executor.tools_config().effective_max_rounds();

        for round in 0..max_rounds {
            if let Some(cancel) = cancellation.as_ref() {
                if cancel.is_cancelled() {
                    if let Some(events) = events.as_ref() {
                        events
                            .progress(&id, ProgressPhase::Cancelling, Some("cancelled".into()))
                            .await;
                    }
                    return ClientResponse::Cancelled {
                        id,
                        turn_id: String::new(),
                        reason: Some("cancelled".into()),
                    };
                }
            }
            if let Some(events) = events.as_ref() {
                events
                    .progress(
                        &id,
                        ProgressPhase::Thinking,
                        Some("planning tool round".into()),
                    )
                    .await;
            }
            match self
                .executor
                .run_one_round(
                    &conversation,
                    &allowed_tools,
                    &tool_ctx,
                    &executed,
                    cancellation.as_ref(),
                )
                .await
            {
                Ok(RoundOutcome::Completed {
                    assistant,
                    executed: round_executed,
                }) => {
                    if let Some(events) = events.as_ref() {
                        self.stream_assistant(events.as_ref(), &id, &assistant.content)
                            .await;
                    }
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
                Ok(RoundOutcome::Cancelled { executed }) => {
                    if let Some(events) = events.as_ref() {
                        events
                            .progress(&id, ProgressPhase::Cancelling, Some("cancelled".into()))
                            .await;
                    }
                    let _ = executed;
                    return ClientResponse::Cancelled {
                        id: id.clone(),
                        turn_id: id,
                        reason: Some("cancelled".into()),
                    };
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

    async fn stream_assistant(&self, events: &dyn TurnEventSink, id: &str, content: &str) {
        if content.is_empty() {
            return;
        }
        let chunk_size = 80usize;
        for chunk in content.as_bytes().chunks(chunk_size) {
            let delta = String::from_utf8_lossy(chunk).into_owned();
            events.assistant_streaming(id, delta).await;
        }
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
