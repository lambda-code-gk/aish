//! リクエスト種別のディスパッチ。

use async_trait::async_trait;
use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::memory_service::MemoryService;
use crate::application::route_turn::RouteTurnService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{parse_tool_names, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError};
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{
    CapabilityPolicy, ContextualMemoryStore, ConversationStore, MemorySpaceResolver,
    MemorySubscriptionBroker, ProfileRegistry, RouterConfig, ShellExecApprovalGate,
    ToolRoundTerminator, ToolsConfig, TurnCancellation, TurnEventSink,
};
use aibe_protocol::{
    ClientRequest, ClientResponse, ErrorCode, MemorySubscribeRequestBody, ProtocolMessage,
    RequestContext,
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
    memory_store: Arc<dyn ContextualMemoryStore>,
    memory_space_resolver: Arc<dyn MemorySpaceResolver>,
    memory_broker: Arc<dyn MemorySubscriptionBroker>,
    capability_policy: Arc<dyn CapabilityPolicy>,
}

impl RequestService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        memory_broker: Arc<dyn MemorySubscriptionBroker>,
    ) -> Self {
        Self::new_with_capability_policy(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            router_profile,
            conversation_store,
            memory_store,
            memory_space_resolver,
            memory_broker,
            crate::adapters::outbound::StaticCapabilityPolicy::local_full(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_capability_policy(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        memory_broker: Arc<dyn MemorySubscriptionBroker>,
        capability_policy: Arc<dyn CapabilityPolicy>,
    ) -> Self {
        Self::new_with_turns_and_capability_policy(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::new(Mutex::new(HashMap::new())),
            router_profile,
            conversation_store,
            memory_store,
            memory_space_resolver,
            memory_broker,
            capability_policy,
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
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        memory_broker: Arc<dyn MemorySubscriptionBroker>,
    ) -> Self {
        Self::new_with_turns_and_capability_policy(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
            memory_store,
            memory_space_resolver,
            memory_broker,
            crate::adapters::outbound::StaticCapabilityPolicy::local_full(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_turns_and_capability_policy(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
        memory_store: Arc<dyn ContextualMemoryStore>,
        memory_space_resolver: Arc<dyn MemorySpaceResolver>,
        memory_broker: Arc<dyn MemorySubscriptionBroker>,
        capability_policy: Arc<dyn CapabilityPolicy>,
    ) -> Self {
        Self {
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
            memory_store,
            memory_space_resolver,
            memory_broker,
            capability_policy,
        }
    }

    pub async fn handle(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
    ) -> ClientResponse {
        self.handle_with_events(request, approval_gate, None, None)
            .await
    }

    pub async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
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
            ClientRequest::MemoryApply(body) => {
                let service = MemoryService::with_capability_policy(
                    Arc::clone(&self.memory_store),
                    Arc::clone(&self.memory_space_resolver),
                    Some(Arc::clone(&self.memory_broker)),
                    Arc::clone(&self.capability_policy),
                );
                service.apply(body.id, body.session_id, &body.context, body.operation)
            }
            ClientRequest::MemoryQuery(body) => {
                let service = MemoryService::with_capability_policy(
                    Arc::clone(&self.memory_store),
                    Arc::clone(&self.memory_space_resolver),
                    None,
                    Arc::clone(&self.capability_policy),
                );
                service.query(body.id, body.session_id, &body.context, body.query)
            }
            ClientRequest::MemoryKindList(body) => {
                let service = MemoryService::with_capability_policy(
                    Arc::clone(&self.memory_store),
                    Arc::clone(&self.memory_space_resolver),
                    None,
                    Arc::clone(&self.capability_policy),
                );
                service.kind_list(body.id, body.session_id, &body.context)
            }
            ClientRequest::MemoryRecipeRun(body) => {
                let service =
                    crate::application::memory_recipe_service::MemoryRecipeService::with_capability_policy(
                        Arc::clone(&self.memory_store),
                        Arc::clone(&self.memory_space_resolver),
                        self.profile_registry.clone(),
                        Some(Arc::clone(&self.memory_broker)),
                        Arc::clone(&self.capability_policy),
                    );
                service
                    .run(
                        body.id,
                        body.session_id,
                        &body.context,
                        &body.recipe,
                        body.apply,
                        body.user_instruction,
                    )
                    .await
            }
            ClientRequest::MemorySubscribe(_) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                "memory_subscribe must use a dedicated connection handler",
            ),
            ClientRequest::AgentTurn {
                id,
                messages,
                tools,
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

                let ctx = match AgentTurnContext::try_from(context) {
                    Ok(ctx) => ctx,
                    Err(e) => match e {},
                };

                let executor = ToolRoundExecutor::new(
                    Arc::clone(llm),
                    Arc::clone(&self.tool_registry),
                    self.tools_config.clone(),
                );
                let agent_turn = AgentTurnService::with_capability_policy(
                    Arc::clone(llm),
                    executor,
                    Arc::clone(&self.terminator),
                    termination_capability,
                    Arc::clone(&self.memory_store),
                    Arc::clone(&self.memory_space_resolver),
                    Arc::clone(&self.capability_policy),
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
                    .run_with_events(
                        id,
                        run_messages.clone(),
                        tools,
                        ctx,
                        approval_gate,
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
    ) -> anyhow::Result<()> {
        use crate::application::memory_subscribe_service::MemorySubscribeService;

        let service = MemorySubscribeService::with_capability_policy(
            Arc::clone(&self.memory_broker),
            Arc::clone(&self.memory_space_resolver),
            Arc::clone(&self.capability_policy),
        );
        let (response, subscription) = service.begin(body.clone());
        MemorySubscribeService::write_response_line(&writer, &response).await?;
        let Some(subscription) = subscription else {
            return Ok(());
        };
        let memory_space_id = match &response {
            ClientResponse::MemorySubscribeResult {
                memory_space_id, ..
            } => memory_space_id.clone(),
            _ => return Ok(()),
        };
        MemorySubscribeService::push_until_disconnect(
            body.id,
            memory_space_id,
            subscription,
            writer,
            lines,
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

#[async_trait]
impl ClientRequestHandler for RequestService {
    async fn handle_with_events(
        &self,
        request: ClientRequest,
        approval_gate: Option<Arc<dyn ShellExecApprovalGate>>,
        events: Option<Arc<dyn TurnEventSink>>,
        cancellation: Option<Arc<TurnCancellation>>,
    ) -> ClientResponse {
        RequestService::handle_with_events(self, request, approval_gate, events, cancellation).await
    }

    async fn handle_memory_subscribe(
        &self,
        body: aibe_protocol::MemorySubscribeRequestBody,
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        lines: Arc<Mutex<crate::ports::inbound::SubscribeConnectionLines>>,
    ) -> anyhow::Result<()> {
        RequestService::handle_memory_subscribe(self, body, writer, lines).await
    }
}
