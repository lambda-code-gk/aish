//! リクエスト種別のディスパッチ。

use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::route_turn::RouteTurnService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{parse_tool_names, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError};
use crate::ports::outbound::{
    ConversationStore, ProfileRegistry, RouterConfig, ShellExecApprovalGate, ToolRoundTerminator,
    ToolsConfig, TurnCancellation, TurnEventSink,
};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, ProtocolMessage, RequestContext};
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
}

impl RequestService {
    pub fn new(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
    ) -> Self {
        Self::new_with_turns(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::new(Mutex::new(HashMap::new())),
            router_profile,
            conversation_store,
        )
    }

    pub fn new_with_turns(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
        router_profile: String,
        conversation_store: Arc<dyn ConversationStore>,
    ) -> Self {
        Self {
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
            router_profile,
            conversation_store,
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
                let agent_turn = AgentTurnService::new(
                    Arc::clone(llm),
                    executor,
                    Arc::clone(&self.terminator),
                    termination_capability,
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
