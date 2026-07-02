//! リクエスト種別のディスパッチ。

use async_trait::async_trait;
use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::route_turn::RouteTurnService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{
    parse_tool_names, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError,
    FeatureEligibilityContext, FeatureRegistry,
};
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    CapabilityPolicy, ClientToolGate, ConversationStore, LlmCallTracer, ProfileRegistry,
    RouterConfig, RpcExtension, ShellExecApprovalGate, ToolRoundTerminator, ToolsConfig,
    TurnCancellation, TurnEventSink, TurnHook,
};
use aibe_protocol::{
    ClientProvidedToolSpec, ClientRequest, ClientResponse, ErrorCode, MemorySubscribeRequestBody,
    ProtocolMessage, RequestContext, ToolRiskClass,
};
use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::application::protocol_convert::ProtocolMessageConversionError;

pub struct RequestService {
    profile_registry: ProfileRegistry,
    tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
    tools_config: ToolsConfig,
    terminator: Arc<dyn ToolRoundTerminator>,
    active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
    router_profile: String,
    conversation_store: Arc<dyn ConversationStore>,
    capability_policy: Arc<dyn CapabilityPolicy>,
    rpc_extension: Arc<dyn RpcExtension>,
    turn_hook: Arc<dyn TurnHook>,
    feature_registry: FeatureRegistry,
    feature_eligibility: FeatureEligibilityContext,
    llm_tracer: Arc<dyn LlmCallTracer>,
}

impl RequestService {
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_turns_and_packs(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
        feature_eligibility: FeatureEligibilityContext,
        llm_tracer: Arc<dyn LlmCallTracer>,
    ) -> Self {
        Self {
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            feature_eligibility,
            llm_tracer,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
    ) -> Self {
        Self::new_with_turns_and_packs(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::new(Mutex::new(HashMap::new())),
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            FeatureEligibilityContext::default(),
            Arc::new(crate::ports::outbound::NoopLlmCallTracer),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_turns(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        capability_policy: Arc<dyn CapabilityPolicy>,
        rpc_extension: Arc<dyn RpcExtension>,
        turn_hook: Arc<dyn TurnHook>,
        feature_registry: FeatureRegistry,
    ) -> Self {
        Self::new_with_turns_and_packs(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
            capability_policy,
            rpc_extension,
            turn_hook,
            feature_registry,
            FeatureEligibilityContext::default(),
            Arc::new(crate::ports::outbound::NoopLlmCallTracer),
        )
    }

    pub async fn handle(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    ) -> ClientResponse {
        self.handle_with_client_tool_gate(request, approval_gate, None)
            .await
    }

    pub async fn handle_with_client_tool_gate(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
    ) -> ClientResponse {
        self.handle_with_events(request, approval_gate, client_tool_gate, None, None)
            .await
    }

    pub async fn cancel_all_active_turns(&self) {
        let guard = self.active_turns.lock().await;
        for cancel in guard.values() {
            cancel.cancel();
        }
    }

    pub async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        match request {
            ClientRequest::Ping { id } => ClientResponse::Pong { id },
            ClientRequest::RouteTurn {
                id,
                query,
                cwd,
                session,
                conversation,
                cli_overrides,
            } => {
                let route_service = RouteTurnService::new(
                    self.profile_registry.clone(),
                    RouterConfig {
                        profile: self.router_profile.clone(),
                    },
                    self.conversation_store.clone(),
                    self.feature_registry.clone(),
                    self.feature_eligibility,
                    Arc::clone(&self.llm_tracer),
                );
                route_service
                    .run(id, query, cwd, session, conversation, cli_overrides)
                    .await
            }
            ClientRequest::CancelTurn { id, turn_id } => {
                let guard = self.active_turns.lock().await;
                if let Some(cancel) = guard.get(&turn_id) {
                    cancel.cancel();
                    ClientResponse::Cancelled {
                        id,
                        turn_id,
                        reason: Some("cancel requested".into()),
                    }
                } else {
                    ClientResponse::Cancelled {
                        id,
                        turn_id,
                        reason: Some("turn not active".into()),
                    }
                }
            }
            ClientRequest::ShellExecApproval { .. } => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "shell_exec_approval must be sent during an active agent_turn",
            ),
            ClientRequest::ToolApproval { .. } => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "tool_approval must be sent during an active agent_turn",
            ),
            ClientRequest::ClientToolResult(_) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "client_tool_result must be sent during an active agent_turn",
            ),
            ClientRequest::MemoryApply(body) => self.rpc_extension.memory_apply(body),
            ClientRequest::MemoryQuery(body) => self.rpc_extension.memory_query(body),
            ClientRequest::MemoryKindList(body) => self.rpc_extension.memory_kind_list(body),
            ClientRequest::MemoryRecipeRun(body) => {
                self.rpc_extension.memory_recipe_run(body).await
            }
            ClientRequest::WorkApply(body) => self.rpc_extension.work_apply(body),
            ClientRequest::WorkQuery(body) => self.rpc_extension.work_query(body),
            ClientRequest::MemorySubscribe(_) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "memory_subscribe must use a dedicated connection handler",
            ),
            ClientRequest::AgentTurn {
                id,
                messages,
                tools,
                client_tools,
                context,
                llm_profile,
            } => {
                let (llm, termination_capability) =
                    match self.profile_registry.resolve(llm_profile.as_deref()) {
                        Ok(pair) => pair,
                        Err(msg) => {
                            return ClientResponse::error(id, ErrorCode::InvalidRequest, msg);
                        }
                    };

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

                if !tools.is_empty() {
                    if let Err(e) = validate_client_cwd_for_tools(&context) {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, e.to_string());
                    }
                }

                let _request_messages = messages.clone();
                let conversation_id = context.conversation_id.clone();
                let ai_session_id = context.ai_session_id.clone();
                let tools = match parse_tool_names(tools) {
                    Ok(names) => names,
                    Err(e) => {
                        return ClientResponse::error(id, ErrorCode::ToolNotAllowed, e.to_string());
                    }
                };
                let client_tools = match validate_client_tools(client_tools) {
                    Ok(tools) => tools,
                    Err(message) => {
                        return ClientResponse::error(id, ErrorCode::InvalidRequest, message);
                    }
                };

                let ctx = match AgentTurnContext::try_from(context) {
                    Ok(ctx) => ctx,
                    Err(e) => match e {},
                };

                let executor = ToolRoundExecutor::new(
                    Arc::clone(llm),
                    Arc::clone(&self.tool_registry),
                    self.tools_config.clone(),
                    Arc::clone(&self.llm_tracer),
                );
                let agent_turn = AgentTurnService::with_capability_policy(
                    Arc::clone(llm),
                    executor,
                    Arc::clone(&self.terminator),
                    termination_capability,
                    Arc::clone(&self.capability_policy),
                    Arc::clone(&self.turn_hook),
                    Arc::clone(&self.llm_tracer),
                );
                let turn_id = id.clone();
                if let Some(cancel) = cancellation.clone() {
                    let mut guard = self.active_turns.lock().await;
                    guard.insert(turn_id.clone(), cancel);
                }
                let mut run_messages = messages.clone();
                if let (Some(session_id), Some(conv_id)) = (&ai_session_id, &conversation_id) {
                    if let Ok(Some(snapshot)) =
                        self.conversation_store.load_snapshot(session_id, conv_id)
                    {
                        if !snapshot.messages.is_empty() {
                            let stored: Result<Vec<ChatMessage>, _> = snapshot
                                .messages
                                .into_iter()
                                .map(ChatMessage::try_from)
                                .collect();
                            if let Ok(stored) = stored {
                                run_messages = stored;
                                run_messages.extend(messages);
                            }
                        }
                    }
                }

                let response = agent_turn
                    .run_with_client_tools_and_events(
                        id,
                        run_messages.clone(),
                        tools,
                        client_tools.clone(),
                        ctx,
                        approval_gate,
                        client_tool_gate,
                        events,
                        cancellation.clone(),
                    )
                    .await;
                if let (Some(conversation_id), Some(ai_session_id)) =
                    (conversation_id, ai_session_id)
                {
                    if let ClientResponse::AgentTurnResult {
                        assistant_message, ..
                    } = &response
                    {
                        let wire_messages: Vec<ProtocolMessage> = run_messages
                            .iter()
                            .map(|m| ProtocolMessage {
                                role: m.role.to_string(),
                                content: m.content.clone(),
                            })
                            .collect();
                        let _ = self.conversation_store.record_turn(
                            &ai_session_id,
                            &conversation_id,
                            current_time_ms(),
                            &wire_messages,
                            assistant_message,
                            None,
                        );
                    }
                }
                if cancellation.is_some() {
                    let mut guard = self.active_turns.lock().await;
                    let _ = guard.remove(&turn_id);
                }
                response
            }
        }
    }

    pub async fn handle_memory_subscribe(
        &self,
        body: MemorySubscribeRequestBody,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
        shutdown: Option<Arc<crate::ports::inbound::ShutdownCoordinator>>,
    ) -> anyhow::Result<()> {
        use crate::application::memory_subscribe_transport::{
            push_memory_subscription_until_disconnect, write_subscribe_response_line,
        };

        let (response, subscription) = self.rpc_extension.memory_subscribe_begin(body.clone());
        write_subscribe_response_line(&writer, &response).await?;
        let Some(subscription) = subscription else {
            return Ok(());
        };
        let memory_space_id = match &response {
            ClientResponse::MemorySubscribeResult {
                memory_space_id, ..
            } => memory_space_id.clone(),
            _ => return Ok(()),
        };
        push_memory_subscription_until_disconnect(
            body.id,
            memory_space_id,
            subscription,
            writer,
            lines,
            shutdown,
        )
        .await
    }
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn validate_client_cwd_for_tools(context: &RequestContext) -> Result<(), ClientCwdError> {
    match context.cwd.as_deref() {
        Some(raw) => ClientCwd::parse(raw).map(|_| ()),
        None => Err(ClientCwdError::Missing),
    }
}

fn validate_client_tools(
    client_tools: Vec<ClientProvidedToolSpec>,
) -> Result<Vec<ClientProvidedToolSpec>, String> {
    let mut out = Vec::new();
    for spec in client_tools {
        if !spec.name.starts_with("aish.") {
            return Err(format!(
                "client tool must use aish. namespace: {}",
                spec.name
            ));
        }
        if spec.risk_class != ToolRiskClass::ReadOnly {
            return Err(format!("client tool must be read_only: {}", spec.name));
        }
        if spec.name != "aish.replay_show" {
            return Err(format!("unsupported client tool: {}", spec.name));
        }
        if spec.max_output_bytes == 0 {
            return Err(format!(
                "client tool max_output_bytes must be > 0: {}",
                spec.name
            ));
        }
        let mut spec = spec;
        spec.max_output_bytes = spec
            .max_output_bytes
            .min(aibe_protocol::MAX_TOOL_OUTPUT_BYTES as u32);
        out.push(spec);
    }
    Ok(out)
}

#[async_trait]
impl ClientRequestHandler for RequestService {
    async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        client_tool_gate: Option<Arc<dyn ClientToolGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        RequestService::handle_with_events(
            self,
            request,
            approval_gate,
            client_tool_gate,
            events,
            cancellation,
        )
        .await
    }

    async fn handle_memory_subscribe(
        &self,
        body: aibe_protocol::MemorySubscribeRequestBody,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
        shutdown: Option<Arc<crate::ports::inbound::ShutdownCoordinator>>,
    ) -> anyhow::Result<()> {
        RequestService::handle_memory_subscribe(self, body, writer, lines, shutdown).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::{ClientProvidedToolSpec, ToolRiskClass};

    #[test]
    fn validate_client_tools_accepts_read_only_aish_namespace() {
        let tools = validate_client_tools(vec![ClientProvidedToolSpec {
            name: "aish.replay_show".into(),
            description: "show".into(),
            parameters: serde_json::json!({"type":"object"}),
            risk_class: ToolRiskClass::ReadOnly,
            max_output_bytes: 8192,
        }])
        .expect("valid");
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn validate_client_tools_rejects_non_namespace_and_dangerous_names() {
        for spec in [
            ClientProvidedToolSpec {
                name: "replay_show".into(),
                description: "show".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            },
            ClientProvidedToolSpec {
                name: "aish.shell_exec".into(),
                description: "shell".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            },
            ClientProvidedToolSpec {
                name: "aish.replay_show".into(),
                description: "show".into(),
                parameters: serde_json::json!({}),
                risk_class: ToolRiskClass::WriteLike,
                max_output_bytes: 8192,
            },
        ] {
            assert!(validate_client_tools(vec![spec]).is_err());
        }
    }
}
