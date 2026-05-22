//! aibe Unix socket クライアントアダプタ（同期 I/O）。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use aibe::protocol::{ClientRequest, ClientResponse, ProtocolMessage, RequestContext};

use crate::domain::AskInput;
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
}

impl AgentClient for AibeUnixClient {
    fn agent_turn(&self, input: &AskInput) -> Result<ClientResponse, AgentError> {
        let mut stream = UnixStream::connect(self.socket_path())
            .map_err(|e| AgentError::Request(format!("connect to aibe: {e}")))?;

        let request = ClientRequest::AgentTurn {
            id: correlation_id(),
            messages: vec![ProtocolMessage {
                role: "user".to_string(),
                content: input.user_message.clone(),
            }],
            tools: vec![],
            context: RequestContext {
                shell_log_tail: input.shell_log_tail.clone(),
            },
        };

        let payload =
            serde_json::to_string(&request).map_err(|e| AgentError::Request(e.to_string()))?;
        writeln!(stream, "{payload}").map_err(|e| AgentError::Request(e.to_string()))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| AgentError::Request(e.to_string()))?;

        serde_json::from_str(line.trim()).map_err(|e| AgentError::Request(e.to_string()))
    }
}

fn correlation_id() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .elapsed()
            .ok()
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}
