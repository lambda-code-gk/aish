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
            tools: input.tools.clone(),
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
