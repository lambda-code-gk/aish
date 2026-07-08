//! aibe Unix socket クライアントアダプタ（`aibe-client` transport 利用）。

use std::path::Path;

use aibe_client::{
    agent_turn_with_client_tools, memory_request as transport_memory_request,
    route_turn as transport_route_turn, AgentTurnCallbacks, AgentTurnProgressEvent, ClientError,
    ClientToolCallRequest, ShellExecApprovalDecision, ToolApprovalDecision,
};

use super::file_write_approval_ui::prompt_file_write_approval;
use super::shell_exec_approval_ui::prompt_shell_exec_approval;
use crate::domain::classify_shell_exec_tier;
use crate::domain::client_tools::replay_show::replay_client_tool_callback;
use aibe_protocol::{
    ClientRequest, ClientResponse, MemoryApplyRequestBody, MemoryContext,
    MemoryKindListRequestBody, MemoryOperationDto, MemoryQueryDto, MemoryQueryRequestBody,
    MemoryRecipeRunRequestBody, ProtocolMessage, WorkApplyRequestBody, WorkOperationDto,
    WorkQueryRequestBody,
};

use crate::domain::AskRequest;
use crate::ports::outbound::{AgentClient, AgentError, MemoryClient, WorkClient};

pub struct AibeUnixClient {
    socket_path: std::path::PathBuf,
}

fn shell_exec_approval_callback(
    prompt: aibe_client::ShellExecApprovalPrompt,
) -> ShellExecApprovalDecision {
    let tier = classify_shell_exec_tier(&prompt.command, &prompt.args);
    let decision = prompt_shell_exec_approval(prompt, tier, false);
    ShellExecApprovalDecision {
        approved: decision.approved,
        approval_origin: decision.approval_origin,
        handoff_result: None,
        handoff_error: None,
    }
}

fn tool_approval_callback(prompt: aibe_client::ToolApprovalPrompt) -> ToolApprovalDecision {
    prompt_file_write_approval(prompt)
}

impl AibeUnixClient {
    pub fn new(socket_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn to_client_request(request: &AskRequest) -> ClientRequest {
        ClientRequest::AgentTurn {
            id: correlation_id(),
            messages: vec![ProtocolMessage {
                role: "user".to_string(),
                content: request.user_message.clone(),
            }],
            tools: request
                .tools
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            client_tools: request.client_tools.clone(),
            context: {
                let mut ctx = request.request_context.clone();
                ctx.shell_log_tail = request.shell_log_tail.clone();
                ctx.cwd = request
                    .client_cwd
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned());
                ctx.ai_session_id = request.ai_session_id.clone();
                ctx.conversation_id = request.conversation_id.clone();
                ctx.into_wire()
            },
            llm_profile: request.llm_profile.clone(),
        }
    }

    pub fn agent_turn_request_stream(
        &self,
        request: ClientRequest,
        on_progress: impl FnMut(AgentTurnProgressEvent),
        on_stream: impl FnMut(String),
        on_approval: impl FnMut(aibe_client::ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
    ) -> Result<ClientResponse, AgentError> {
        agent_turn_with_client_tools(
            self.socket_path(),
            request,
            on_progress,
            on_stream,
            |_| None,
            AgentTurnCallbacks::new(on_approval, tool_approval_callback),
        )
        .map_err(map_client_error)
    }

    pub fn agent_turn_request_stream_with_client_tools(
        &self,
        request: ClientRequest,
        on_progress: impl FnMut(AgentTurnProgressEvent),
        on_stream: impl FnMut(String),
        on_client_tool: impl FnMut(ClientToolCallRequest) -> Option<aibe_protocol::ClientToolResult>,
        on_approval: impl FnMut(aibe_client::ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
    ) -> Result<ClientResponse, AgentError> {
        agent_turn_with_client_tools(
            self.socket_path(),
            request,
            on_progress,
            on_stream,
            on_client_tool,
            AgentTurnCallbacks::new(on_approval, tool_approval_callback),
        )
        .map_err(map_client_error)
    }

    pub fn route_turn(&self, request: ClientRequest) -> Result<ClientResponse, AgentError> {
        transport_route_turn(self.socket_path(), request).map_err(map_client_error)
    }

    pub fn memory_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: MemoryOperationDto,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_apply_request(session_id, context, operation))
    }

    pub fn memory_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
        query: MemoryQueryDto,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_query_request(session_id, context, query))
    }

    pub fn memory_kind_list(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_kind_list_request(session_id, context))
    }

    pub fn memory_recipe_run(
        &self,
        session_id: &str,
        context: &MemoryContext,
        recipe: &str,
        apply: bool,
        user_instruction: Option<String>,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_recipe_run_request(
            session_id,
            context,
            recipe,
            apply,
            user_instruction,
        ))
    }

    fn send_memory_request(&self, request: ClientRequest) -> Result<ClientResponse, AgentError> {
        transport_memory_request(self.socket_path(), request).map_err(map_client_error)
    }

    pub fn agent_turn_stream(
        &self,
        request: &AskRequest,
        on_progress: impl FnMut(AgentTurnProgressEvent),
        on_stream: impl FnMut(String),
        on_approval: impl FnMut(aibe_client::ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
    ) -> Result<ClientResponse, AgentError> {
        self.agent_turn_request_stream(
            Self::to_client_request(request),
            on_progress,
            on_stream,
            on_approval,
        )
    }

    pub fn cancel_turn(&self, turn_id: &str) -> Result<(), AgentError> {
        aibe_client::cancel_turn(self.socket_path(), correlation_id(), turn_id)
            .map_err(map_client_error)
    }
}

impl AgentClient for AibeUnixClient {
    fn agent_turn(&self, request: &AskRequest) -> Result<ClientResponse, AgentError> {
        let wire = Self::to_client_request(request);
        if request.client_tools.is_empty() {
            return agent_turn_with_client_tools(
                self.socket_path(),
                wire,
                |_| {},
                |_| {},
                |_| None,
                AgentTurnCallbacks::new(shell_exec_approval_callback, tool_approval_callback),
            )
            .map_err(map_client_error);
        }
        agent_turn_with_client_tools(
            self.socket_path(),
            wire,
            |_| {},
            |_| {},
            replay_client_tool_callback(request.replay_events.clone()),
            AgentTurnCallbacks::new(shell_exec_approval_callback, tool_approval_callback),
        )
        .map_err(map_client_error)
    }
}

impl MemoryClient for AibeUnixClient {
    fn memory_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: MemoryOperationDto,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_apply_request(session_id, context, operation))
    }

    fn memory_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
        query: MemoryQueryDto,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_query_request(session_id, context, query))
    }

    fn memory_kind_list(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_kind_list_request(session_id, context))
    }

    fn memory_recipe_run(
        &self,
        session_id: &str,
        context: &MemoryContext,
        recipe: &str,
        apply: bool,
        user_instruction: Option<String>,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(memory_recipe_run_request(
            session_id,
            context,
            recipe,
            apply,
            user_instruction,
        ))
    }
}

impl WorkClient for AibeUnixClient {
    fn work_query(
        &self,
        session_id: &str,
        context: &MemoryContext,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(ClientRequest::WorkQuery(WorkQueryRequestBody {
            id: correlation_id(),
            session_id: session_id.to_string(),
            context: context.clone(),
        }))
    }

    fn work_apply(
        &self,
        session_id: &str,
        context: &MemoryContext,
        operation: WorkOperationDto,
    ) -> Result<ClientResponse, AgentError> {
        self.send_memory_request(ClientRequest::WorkApply(WorkApplyRequestBody {
            id: correlation_id(),
            session_id: session_id.to_string(),
            context: context.clone(),
            operation,
        }))
    }
}

fn memory_apply_request(
    session_id: &str,
    context: &MemoryContext,
    operation: MemoryOperationDto,
) -> ClientRequest {
    ClientRequest::MemoryApply(MemoryApplyRequestBody {
        id: correlation_id(),
        session_id: session_id.to_string(),
        context: context.clone(),
        operation,
    })
}

fn memory_query_request(
    session_id: &str,
    context: &MemoryContext,
    query: MemoryQueryDto,
) -> ClientRequest {
    ClientRequest::MemoryQuery(MemoryQueryRequestBody {
        id: correlation_id(),
        session_id: session_id.to_string(),
        context: context.clone(),
        query,
    })
}

fn memory_kind_list_request(session_id: &str, context: &MemoryContext) -> ClientRequest {
    ClientRequest::MemoryKindList(MemoryKindListRequestBody {
        id: correlation_id(),
        session_id: session_id.to_string(),
        context: context.clone(),
    })
}

fn memory_recipe_run_request(
    session_id: &str,
    context: &MemoryContext,
    recipe: &str,
    apply: bool,
    user_instruction: Option<String>,
) -> ClientRequest {
    ClientRequest::MemoryRecipeRun(MemoryRecipeRunRequestBody {
        id: correlation_id(),
        session_id: session_id.to_string(),
        context: context.clone(),
        recipe: recipe.to_string(),
        apply,
        user_instruction,
    })
}

fn map_client_error(e: ClientError) -> AgentError {
    match e {
        ClientError::Connect(io) => AgentError::Request(format!("connect to aibe: {io}")),
        ClientError::Serialize(msg)
        | ClientError::Deserialize(msg)
        | ClientError::Unexpected(msg) => AgentError::Request(msg),
    }
}

fn correlation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}-{}", std::process::id(), seq, nanos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::RequestContextInput;
    use aibe_protocol::{ClientRequest, ToolName};

    #[test]
    fn to_client_request_preserves_pre_resolved_context() {
        let request = AskRequest {
            user_message: "hi".into(),
            shell_log_tail: Some("tail".into()),
            client_cwd: Some("/tmp".into()),
            tools: vec![ToolName::read_file()],
            llm_profile: None,
            client_tools: vec![],
            ai_session_id: Some("sess".into()),
            conversation_id: Some("conv".into()),
            replay_events: vec![],
            replay_manifest_block: None,
            request_context: RequestContextInput {
                system_instruction: Some("be brief".into()),
                ..Default::default()
            },
        };
        let ClientRequest::AgentTurn { context, .. } = AibeUnixClient::to_client_request(&request)
        else {
            panic!("expected agent_turn");
        };
        assert_eq!(context.system_instruction.as_deref(), Some("be brief"));
        assert_eq!(context.shell_log_tail.as_deref(), Some("tail"));
        assert_eq!(context.cwd.as_deref(), Some("/tmp"));
    }
}
