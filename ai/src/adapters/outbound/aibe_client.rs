//! aibe Unix socket クライアントアダプタ（`aibe-client` transport 利用）。

use std::path::Path;

use aibe_client::{agent_turn as transport_agent_turn, ClientError, ShellExecApprovalPrompt};
use aibe_protocol::{ClientRequest, ClientResponse, ProtocolMessage, RequestContext};

use crate::domain::AskRequest;
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
            context: RequestContext {
                shell_log_tail: request.shell_log_tail.clone(),
                cwd: request
                    .client_cwd
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
            },
            llm_profile: request.llm_profile.clone(),
        }
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

fn prompt_shell_exec_approval(prompt: ShellExecApprovalPrompt) -> bool {
    eprintln!("ai: shell_exec approval required:");
    eprintln!("  command: {}", prompt.command);
    if prompt.args.is_empty() {
        eprintln!("  args: (none)");
    } else {
        eprintln!("  args: {}", prompt.args.join(" "));
    }
    eprint!("Execute? [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let mut line = String::new();
    let Ok(n) = std::io::stdin().read_line(&mut line) else {
        eprintln!("ai: shell_exec denied (stdin unavailable)");
        return false;
    };
    if n == 0 {
        eprintln!("ai: shell_exec denied (non-interactive stdin)");
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
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
