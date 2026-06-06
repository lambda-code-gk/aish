//! リクエスト種別のディスパッチ。

use std::sync::Arc;

use crate::application::agent_turn::AgentTurnService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::domain::{parse_tool_names, AgentTurnContext, ChatMessage, ClientCwd, ClientCwdError};
use crate::ports::outbound::{
    ProfileRegistry, ShellExecApprovalGate, ToolRoundTerminator, ToolsConfig, TurnCancellation,
    TurnEventSink,
};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, RequestContext};
use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::application::protocol_convert::ProtocolMessageConversionError;

pub struct RequestService {
    profile_registry: ProfileRegistry,
    tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
    tools_config: ToolsConfig,
    terminator: Arc<dyn ToolRoundTerminator>,
    active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
}

impl RequestService {
    pub fn new(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
    ) -> Self {
        Self::new_with_turns(
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            Arc::new(Mutex::new(HashMap::new())),
        )
    }

    pub fn new_with_turns(
        profile_registry: ProfileRegistry,
        tool_registry: Arc<dyn crate::ports::outbound::ToolRegistry>,
        tools_config: ToolsConfig,
        terminator: Arc<dyn ToolRoundTerminator>,
        active_turns: Arc<Mutex<HashMap<String, Arc<TurnCancellation>>>>,
    ) -> Self {
        Self {
            profile_registry,
            tool_registry,
            tools_config,
            terminator,
            active_turns,
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
                let response = agent_turn
                    .run_with_events(
                        id,
                        messages,
                        tools,
                        ctx,
                        approval_gate,
                        events,
                        cancellation.clone(),
                    )
                    .await;
                if cancellation.is_some() {
                    let mut guard = self.active_turns.lock().await;
                    let _ = guard.remove(&turn_id);
                }
                response
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
