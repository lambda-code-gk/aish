//! aibe Unix socket クライアントアダプタ（`aibe-client` transport 利用）。

use std::path::Path;

use aibe_client::{
    agent_turn as transport_agent_turn, agent_turn_with_events, route_turn as transport_route_turn,
    send_cancel_request, AgentTurnProgressEvent, ClientError,
};

use super::shell_exec_approval_ui::prompt_shell_exec_approval;
use aibe_protocol::{ClientRequest, ClientResponse, ProtocolMessage};

use super::terminal_size::detect_terminal_size;
use crate::domain::{AskRequest, RequestContextInput};
use crate::ports::outbound::{AgentClient, AgentError};

pub struct AibeUnixClient {
    socket_path: std::path::PathBuf,
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
            context: RequestContextInput {
                shell_log_tail: request.shell_log_tail.clone(),
                cwd: request
                    .client_cwd
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                ai_session_id: request.ai_session_id.clone(),
                conversation_id: request.conversation_id.clone(),
                ..Default::default()
            }
            .with_console_system_instruction(detect_terminal_size())
            .into_wire(),
            llm_profile: request.llm_profile.clone(),
        }
    }

    pub fn agent_turn_request_stream(
        &self,
        request: ClientRequest,
        on_progress: impl FnMut(AgentTurnProgressEvent),
        on_stream: impl FnMut(String),
        on_approval: impl FnMut(aibe_client::ShellExecApprovalPrompt) -> bool,
    ) -> Result<ClientResponse, AgentError> {
        agent_turn_with_events(
            self.socket_path(),
            request,
            on_progress,
            on_stream,
            on_approval,
        )
        .map_err(map_client_error)
    }

    pub fn route_turn(&self, request: ClientRequest) -> Result<ClientResponse, AgentError> {
        transport_route_turn(self.socket_path(), request).map_err(map_client_error)
    }

    pub fn agent_turn_stream(
        &self,
        request: &AskRequest,
        on_progress: impl FnMut(AgentTurnProgressEvent),
        on_stream: impl FnMut(String),
        on_approval: impl FnMut(aibe_client::ShellExecApprovalPrompt) -> bool,
    ) -> Result<ClientResponse, AgentError> {
        self.agent_turn_request_stream(
            Self::to_client_request(request),
            on_progress,
            on_stream,
            on_approval,
        )
    }

    pub fn cancel_turn(&self, turn_id: &str) -> Result<(), AgentError> {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(self.socket_path())
            .map_err(|e| AgentError::Request(format!("connect to aibe: {e}")))?;
        send_cancel_request(&mut stream, correlation_id(), turn_id.to_string())
            .map_err(|e| AgentError::Request(format!("cancel turn: {e}")))?;
        stream
            .flush()
            .map_err(|e| AgentError::Request(format!("cancel flush: {e}")))?;
        Ok(())
    }
}

impl AgentClient for AibeUnixClient {
    fn agent_turn(&self, request: &AskRequest) -> Result<ClientResponse, AgentError> {
        let wire = Self::to_client_request(request);
        transport_agent_turn(self.socket_path(), wire, prompt_shell_exec_approval)
            .map_err(map_client_error)
    }
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
