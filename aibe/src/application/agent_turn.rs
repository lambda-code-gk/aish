//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use crate::application::tool_round_terminator::finish_after_max_tool_rounds;
use crate::domain::{AgentTurnContext, Capability, ChatMessage, ToolName, SHELL_EXEC};
use crate::ports::outbound::{
    CapabilityPolicy, ContextualMemoryStore, LlmProvider, MemorySpaceResolver,
    ShellExecApprovalGate, TerminationCapability, ToolExecutionContext, ToolRoundTerminator,
    TurnCancellation, TurnEventSink,
};
use aibe_protocol::MEMORY_PROMPT_BUDGET_BYTES;
use aibe_protocol::{AgentTurnStatus, ClientResponse, ErrorCode, ProgressPhase};

use crate::application::protocol_convert::protocol_message_out_from_chat;

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
    executor: ToolRoundExecutor,
    terminator: Arc<dyn ToolRoundTerminator>,
    termination_capability: TerminationCapability,
    memory_store: Arc<dyn ContextualMemoryStore>,
    memory_space_resolver: Arc<dyn MemorySpaceResolver>,
    capability_policy: Arc<dyn CapabilityPolicy>,
}

impl AgentTurnService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
    ) -> Self {
        Self::with_capability_policy(
            llm,
            executor,
            terminator,
            termination_capability,
            memory_store,
            memory_space_resolver,
            crate::adapters::outbound::StaticCapabilityPolicy::local_full(),
        )
    }

    pub fn with_capability_policy(
        llm: Arc<dyn LlmProvider>,
        executor: ToolRoundExecutor,
        terminator: Arc<dyn ToolRoundTerminator>,
        termination_capability: TerminationCapability,
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        capability_policy: Arc<dyn CapabilityPolicy>,
    ) -> Self {
        Self {
            llm,
            executor,
            terminator,
            termination_capability,
            memory_store,
            memory_space_resolver,
            capability_policy,
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

        if let Err(denied) = self.capability_policy.require(Capability::AgentAsk) {
            return ClientResponse::error(id, ErrorCode::InvalidRequest, denied.message());
        }
        if tools.iter().any(|t| t.as_str() == SHELL_EXEC) {
            if let Err(denied) = self.capability_policy.require(Capability::ShellPropose) {
                return ClientResponse::error(id, ErrorCode::InvalidRequest, denied.message());
            }
        }

        let conversation = prepare_turn_messages(
            messages,
            &context,
            self.memory_store.as_ref(),
            self.memory_space_resolver.as_ref(),
        );

        if tools.is_empty() {
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

        let mut tool_ctx = ToolExecutionContext::new(
            context
                .client_cwd
                .clone()
                .expect("validate_tools_enabled ensures cwd"),
        )
        .with_turn_id(id.clone())
        .with_capability_policy(Arc::clone(&self.capability_policy));
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
                    ProgressPhase::Thinking,
                    Some("generating response".into()),
                )
                .await;
        }

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

/// turn 用メッセージの前置（aibe application 内の注入箇所）。
fn prepare_turn_messages(
    mut messages: Vec<ChatMessage>,
    context: &AgentTurnContext,
    memory_store: &dyn ContextualMemoryStore,
    memory_space_resolver: &dyn MemorySpaceResolver,
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
        insert_at += 1;
    }
    if let Some(ref session_id) = context.ai_session_id {
        let user_query = messages
            .iter()
            .rev()
            .find(|m| m.role == crate::domain::MessageRole::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let cwd = context.client_cwd.as_ref().map(|c| c.as_path());
        let block = memory_space_resolver
            .resolve_for_turn(session_id, context.memory_space_id.as_deref(), cwd)
            .ok()
            .and_then(|store_ctx| {
                memory_store
                    .resolve_for_prompt(&store_ctx, user_query, MEMORY_PROMPT_BUDGET_BYTES)
                    .ok()
            });
        if let Some(block) = block {
            if !block.content.is_empty() {
                messages.insert(insert_at, ChatMessage::user(block.content));
            }
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ClientCwd, MemoryBlock, ShellLogTail};
    use crate::ports::outbound::{
        ContextualMemoryStore, ContextualMemoryStoreError, MemorySpaceResolver,
    };
    use aibe_protocol::MemoryContext;

    struct NoopMemoryStore;

    impl ContextualMemoryStore for NoopMemoryStore {
        fn apply(
            &self,
            _ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _operation: &aibe_protocol::MemoryOperationDto,
            _now_ms: u64,
        ) -> Result<Vec<crate::domain::MemoryEntry>, ContextualMemoryStoreError> {
            Ok(vec![])
        }

        fn query(
            &self,
            _ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _query: &aibe_protocol::MemoryQueryDto,
        ) -> Result<Vec<crate::domain::MemoryEntry>, ContextualMemoryStoreError> {
            Ok(vec![])
        }

        fn resolve_for_prompt(
            &self,
            _ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _user_query: &str,
            _budget_bytes: usize,
        ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
            Ok(MemoryBlock {
                content: String::new(),
            })
        }
    }

    struct NoopMemorySpaceResolver;

    impl MemorySpaceResolver for NoopMemorySpaceResolver {
        fn resolve_store_context<'a>(
            &self,
            session_id: &'a str,
            _context: &MemoryContext,
            cwd_path: Option<&'a std::path::Path>,
        ) -> Result<crate::ports::outbound::MemoryStoreContext<'a>, ContextualMemoryStoreError>
        {
            Ok(crate::ports::outbound::MemoryStoreContext {
                session_id,
                memory_space_id: "test".into(),
                cwd: cwd_path,
            })
        }

        fn resolve_for_turn<'a>(
            &self,
            session_id: &'a str,
            explicit_memory_space_id: Option<&str>,
            cwd_path: Option<&'a std::path::Path>,
        ) -> Result<crate::ports::outbound::MemoryStoreContext<'a>, ContextualMemoryStoreError>
        {
            Ok(crate::ports::outbound::MemoryStoreContext {
                session_id,
                memory_space_id: explicit_memory_space_id.unwrap_or("test").to_string(),
                cwd: cwd_path,
            })
        }
    }

    /// 解決された memory_space_id をそのまま block にして返す store。
    struct EchoSpaceMemoryStore;

    impl ContextualMemoryStore for EchoSpaceMemoryStore {
        fn apply(
            &self,
            _ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _operation: &aibe_protocol::MemoryOperationDto,
            _now_ms: u64,
        ) -> Result<Vec<crate::domain::MemoryEntry>, ContextualMemoryStoreError> {
            Ok(vec![])
        }

        fn query(
            &self,
            _ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _query: &aibe_protocol::MemoryQueryDto,
        ) -> Result<Vec<crate::domain::MemoryEntry>, ContextualMemoryStoreError> {
            Ok(vec![])
        }

        fn resolve_for_prompt(
            &self,
            ctx: &crate::ports::outbound::MemoryStoreContext<'_>,
            _user_query: &str,
            _budget_bytes: usize,
        ) -> Result<MemoryBlock, ContextualMemoryStoreError> {
            Ok(MemoryBlock {
                content: format!("[memory from {}]", ctx.memory_space_id),
            })
        }
    }

    fn empty_memory() -> NoopMemoryStore {
        NoopMemoryStore
    }

    fn noop_resolver() -> NoopMemorySpaceResolver {
        NoopMemorySpaceResolver
    }

    #[test]
    fn prepare_turn_messages_uses_explicit_memory_space_id() {
        let mut ctx = AgentTurnContext::for_text_only(None);
        ctx.ai_session_id = Some("sess_001".into());
        ctx.memory_space_id = Some("ctx_a".into());
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &EchoSpaceMemoryStore,
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "[memory from ctx_a]");
    }

    #[test]
    fn prepare_turn_messages_injects_memory_without_cwd() {
        let mut ctx = AgentTurnContext::for_text_only(None);
        ctx.ai_session_id = Some("sess_001".into());
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &EchoSpaceMemoryStore,
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "[memory from test]");
    }

    #[test]
    fn prepare_turn_messages_skips_empty_normalized_tail() {
        let ctx = AgentTurnContext::for_text_only(None);
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &empty_memory(),
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hi");
    }

    #[test]
    fn prepare_turn_messages_prepends_shell_log_tail() {
        let tail = ShellLogTail::from_wire_opt("log line").expect("tail");
        let ctx = AgentTurnContext::for_text_only(Some(tail));
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &empty_memory(),
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].content.starts_with("[shell log tail]\n"));
        assert!(msgs[0].content.contains("log line"));
        assert_eq!(msgs[1].content, "hi");
    }

    #[test]
    fn prepare_turn_messages_prepends_client_system_instruction() {
        let mut ctx = AgentTurnContext::for_text_only(None);
        ctx.system_instruction = Some("be brief".into());
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &empty_memory(),
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, crate::domain::MessageRole::System);
        assert_eq!(msgs[0].content, "be brief");
        assert_eq!(msgs[1].content, "hi");
    }

    #[test]
    fn prepare_turn_messages_orders_system_before_shell_log_tail() {
        let tail = ShellLogTail::from_wire_opt("log line").expect("tail");
        let mut ctx = AgentTurnContext::for_text_only(Some(tail));
        ctx.system_instruction = Some("be brief".into());
        let msgs = prepare_turn_messages(
            vec![ChatMessage::user("hi")],
            &ctx,
            &empty_memory(),
            &noop_resolver(),
        );
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].role, crate::domain::MessageRole::System);
        assert!(msgs[1].content.starts_with("[shell log tail]\n"));
        assert_eq!(msgs[2].content, "hi");
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
