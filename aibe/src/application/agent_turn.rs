//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;
use std::time::Instant;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{AgentTurnContext, Capability, ChatMessage, ToolName, SHELL_EXEC};
use crate::ports::outbound::{
    CapabilityPolicy, ClientToolGate, LlmCallTracer, LlmProvider, ShellExecApprovalGate,
    TerminationCapability, ToolApprovalGate, ToolExecutionContext, ToolRoundTerminator,
    TurnCancellation, TurnEventSink, TurnHook,
};
use aibe_protocol::{
    AgentTurnStatus, ClientProvidedToolSpec, ClientResponse, ErrorCode, ProgressPhase,
};

use crate::application::protocol_convert::protocol_message_out_from_chat;

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
    executor: ToolRoundExecutor,
    terminator: Arc<dyn ToolRoundTerminator>,
    termination_capability: TerminationCapability,
    capability_policy: Arc<dyn CapabilityPolicy>,
    turn_hook: Arc<dyn TurnHook>,
    llm_tracer: Arc<dyn LlmCallTracer>,
}

impl AgentTurnService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
        capability_policy: Arc<dyn CapabilityPolicy>,
        turn_hook: Arc<dyn TurnHook>,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self::with_capability_policy(
            llm,
            executor,
            terminator,
            termination_capability,
            capability_policy,
            turn_hook,
            llm_tracer,
        )
    }

    pub fn with_capability_policy(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
        capability_policy: Arc<dyn CapabilityPolicy>,
        turn_hook: Arc<dyn TurnHook>,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self {
            llm,
            executor,
            terminator,
            termination_capability,
            capability_policy,
            turn_hook,
            llm_tracer,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        context: AgentTurnContext,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    ) -> ClientResponse {
        self.run_with_client_tools(
            id,
            messages,
            tools,
            Vec::new(),
            context,
            approval_gate,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_client_tools(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        client_tools: Vec<ClientProvidedToolSpec>,
        context: AgentTurnContext,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        tool_approval_gate: Option<Arc<dyn ToolApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
    ) -> ClientResponse {
        self.run_with_client_tools_and_events(
            id,
            messages,
            tools,
            client_tools,
            context,
            approval_gate,
            tool_approval_gate,
            client_tool_gate,
            None,
            None,
        )
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
        self.run_with_client_tools_and_events(
            id,
            messages,
            tools,
            Vec::new(),
            context,
            approval_gate,
            None,
            None,
            events,
            cancellation,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_client_tools_and_events(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<ToolName>,
        client_tools: Vec<ClientProvidedToolSpec>,
        context: AgentTurnContext,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        tool_approval_gate: Option<Arc<dyn ToolApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
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

        if let Err(denied) = self.capability_policy.require(Capability::AgentAsk) {
            return ClientResponse::error(id, ErrorCode::InvalidRequest, denied.message());
        }
        if tools.iter().any(|t| t.as_str() == SHELL_EXEC) {
            if let Err(denied) = self.capability_policy.require(Capability::ShellPropose) {
                return ClientResponse::error(id, ErrorCode::InvalidRequest, denied.message());
            }
        }

        let prepped = prepend_system_and_shell(messages, &context);
        if let Some(events) = events.as_ref() {
            events
                .progress(
                    &id,
                    ProgressPhase::Preparing,
                    Some("preparing context".into()),
                )
                .await;
        }
        let conversation = self
            .turn_hook
            .prepare_turn_messages(&context, prepped.clone())
            .unwrap_or(prepped);

        if tools.is_empty() && client_tools.is_empty() {
            return self
                .finish_text_only(
                    id,
                    &conversation,
                    events.clone(),
                    cancellation.as_ref().map(Arc::as_ref),
                )
                .await;
        }

        if let Err(e) = context.validate_tools_enabled(&tools) {
            return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
        }

        let Some(client_cwd) = context.client_cwd.clone() else {
            let message = if tools.is_empty() {
                "cwd is required when client tools are advertised"
            } else {
                "cwd is required when tools are enabled"
            };
            return ClientResponse::error(id, ErrorCode::InvalidRequest, message);
        };

        let mut tool_ctx = ToolExecutionContext::new(client_cwd)
            .with_turn_id(id.clone())
            .with_collaborative_handoff(context.collaborative_handoff)
            .with_capability_policy(Arc::clone(&self.capability_policy));
        if let Some(gate) = approval_gate {
            tool_ctx = tool_ctx.with_approval_gate(gate);
        }
        if let Some(gate) = tool_approval_gate {
            tool_ctx = tool_ctx.with_tool_approval_gate(gate);
        }
        if let Some(gate) = client_tool_gate {
            tool_ctx = tool_ctx.with_client_tool_gate(gate);
        }
        tool_ctx = tool_ctx.with_client_tools(client_tools.clone());

        self.run_with_tools(
            id,
            conversation,
            tools,
            client_tools,
            tool_ctx,
            events,
            cancellation,
        )
        .await
    }

    async fn finish_text_only(
        &self,
        id: String,
        conversation: &[ChatMessage],
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<&TurnCancellation>,
    ) -> ClientResponse {
        if let Some(cancel) = cancellation {
            if cancel.is_cancelled() {
                if let Some(events) = events.as_ref() {
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
        if let Some(events) = events.as_ref() {
            events
                .progress(
                    &id,
                    ProgressPhase::Generating,
                    Some("generating response".into()),
                )
                .await;
        }
        self.llm_tracer.start("agent_turn", None, None);
        let started = Instant::now();

        let assistant = if let Some(cancel) = cancellation {
            if let Some(events) = events.as_ref() {
                let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let forwarder_events = Arc::clone(events);
                let turn_id = id.clone();
                let stream_forwarder = tokio::spawn(async move {
                    while let Some(delta) = delta_rx.recv().await {
                        forwarder_events.assistant_streaming(&turn_id, delta).await;
                    }
                });
                let mut on_delta = |delta: String| {
                    let _ = delta_tx.send(delta);
                };
                let assistant = tokio::select! {
                    res = self.llm.complete_streaming(conversation, &mut on_delta) => res,
                    _ = cancel.wait() => {
                        drop(delta_tx);
                        stream_forwarder.abort();
                        self.llm_tracer.end("agent_turn", started.elapsed().as_millis() as u64, false);
                        events
                            .progress(
                                &id,
                                ProgressPhase::Cancelling,
                                Some("cancelled".into()),
                            )
                            .await;
                        return ClientResponse::Cancelled {
                            id: id.clone(),
                            turn_id: id,
                            reason: Some("cancelled".into()),
                        };
                    }
                };
                drop(delta_tx);
                if let Some(handle) = Some(stream_forwarder) {
                    let _ = handle.await;
                }
                assistant
            } else {
                let mut ignore_delta = |_delta: String| {};
                tokio::select! {
                    res = self.llm.complete_streaming(conversation, &mut ignore_delta) => res,
                    _ = cancel.wait() => {
                        self.llm_tracer.end("agent_turn", started.elapsed().as_millis() as u64, false);
                        return ClientResponse::Cancelled {
                            id: id.clone(),
                            turn_id: id,
                            reason: Some("cancelled".into()),
                        };
                    }
                }
            }
        } else if let Some(events) = events.as_ref() {
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let forwarder_events = Arc::clone(events);
            let turn_id = id.clone();
            let stream_forwarder = tokio::spawn(async move {
                while let Some(delta) = delta_rx.recv().await {
                    forwarder_events.assistant_streaming(&turn_id, delta).await;
                }
            });
            let mut on_delta = |delta: String| {
                let _ = delta_tx.send(delta);
            };
            let assistant = self
                .llm
                .complete_streaming(conversation, &mut on_delta)
                .await;
            drop(delta_tx);
            if let Some(handle) = Some(stream_forwarder) {
                let _ = handle.await;
            }
            assistant
        } else {
            let mut ignore_delta = |_delta: String| {};
            self.llm
                .complete_streaming(conversation, &mut ignore_delta)
                .await
        };
        self.llm_tracer.end(
            "agent_turn",
            started.elapsed().as_millis() as u64,
            assistant.is_ok(),
        );

        if let Some(events) = events.as_ref() {
            events.final_response(&id).await;
        }

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

    #[allow(clippy::too_many_arguments)]
    async fn run_with_tools(
        &self,
        id: String,
        mut conversation: Vec<ChatMessage>,
        allowed_tools: Vec<ToolName>,
        client_tools: Vec<ClientProvidedToolSpec>,
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
                    &client_tools,
                    &tool_ctx,
                    &executed,
                    events.clone(),
                    cancellation.as_ref(),
                )
                .await
            {
                Ok(RoundOutcome::Completed {
                    assistant,
                    executed: round_executed,
                }) => {
                    if let Some(events) = events.as_ref() {
                        events.final_response(&id).await;
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
}

/// system instruction と shell log tail を前置する（memory 注入は `TurnHook` が担当）。
fn prepend_system_and_shell(
    mut messages: Vec<ChatMessage>,
    context: &AgentTurnContext,
) -> Vec<ChatMessage> {
    let mut insert_at = 0;
    if let Some(ref instruction) = context.system_instruction {
        messages.insert(insert_at, ChatMessage::system(instruction.clone()));
        insert_at += 1;
    }
    if let Some(ref tail) = context.shell_log_tail {
        messages.insert(
            insert_at,
            ChatMessage::user(format!("[shell log tail]\n{}", tail.as_str())),
        );
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;

    #[test]
    fn validate_tools_enabled_rejects_missing_cwd() {
        let ctx = AgentTurnContext::for_text_only(None);
        assert!(ctx
            .validate_tools_enabled(&[ToolName::read_file()])
            .is_err());
    }

    #[test]
    fn prepend_system_and_shell_orders_system_before_shell_log_tail() {
        use crate::domain::{MessageRole, ShellLogTail};
        let tail = ShellLogTail::from_wire_opt("log line").expect("tail");
        let mut ctx = AgentTurnContext::for_text_only(Some(tail));
        ctx.system_instruction = Some("be brief".into());
        let msgs = prepend_system_and_shell(vec![ChatMessage::user("hi")], &ctx);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, MessageRole::System);
        assert!(msgs[1].content.starts_with("[shell log tail]\n"));
        assert_eq!(msgs[2].content, "hi");
    }

    #[test]
    fn validate_tools_enabled_accepts_absolute_cwd() {
        let cwd = ClientCwd::parse("/tmp/proj").expect("cwd");
        let ctx = AgentTurnContext::for_tool_turn(cwd, None);
        assert!(ctx.validate_tools_enabled(&[ToolName::read_file()]).is_ok());
    }
}
