//! Unix domain socket の bind / accept / NDJSON フレーミング。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::adapters::inbound::connection_approval::ConnectionApprovalGate;
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::outbound::{ShellExecApprovalGate, TurnCancellation, TurnEventSink};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, ProgressPhase};

pub async fn run(
    socket_path: PathBuf,
    handler: Arc<dyn ClientRequestHandler>,
) -> anyhow::Result<()> {
    prepare_socket_path(&socket_path)?;
    let listener = bind_unix_listener(&socket_path)?;
    eprintln!("aibe: listening on {}", socket_path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            if let Err(e) = serve_connection(stream, handler).await {
                eprintln!("aibe: connection error: {e}");
            }
        });
    }
}

fn prepare_socket_path(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn bind_unix_listener(path: &Path) -> anyhow::Result<UnixListener> {
    let old_umask = unsafe { libc::umask(0o077) };
    let result = UnixListener::bind(path).and_then(|listener| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    });
    unsafe {
        libc::umask(old_umask);
    }
    result.map_err(Into::into)
}

async fn serve_connection(
    stream: UnixStream,
    handler: Arc<dyn ClientRequestHandler>,
) -> anyhow::Result<()> {
    let (reader, writer) = stream.into_split();
    let writer = Arc::new(Mutex::new(writer));
    let lines = Arc::new(Mutex::new(BufReader::new(reader).lines()));

    loop {
        let line = {
            let mut guard = lines.lock().await;
            match guard.next_line().await? {
                Some(line) => line,
                None => break,
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<ClientRequest>(line) {
            Ok(req) => {
                if let ClientRequest::MemorySubscribe(body) = req {
                    handler
                        .handle_memory_subscribe(body, Arc::clone(&writer), Arc::clone(&lines))
                        .await?;
                    break;
                }
                let mut cancellation: Option<Arc<TurnCancellation>> = None;
                let mut gate: Option<Arc<dyn ShellExecApprovalGate>> = None;
                let mut events: Option<Arc<dyn TurnEventSink>> = None;
                if let ClientRequest::AgentTurn { id, .. } = &req {
                    let cancel = Arc::new(TurnCancellation::new());
                    let sink: Arc<dyn TurnEventSink> = Arc::new(ConnectionEventSink {
                        writer: Arc::clone(&writer),
                    });
                    let approval_gate: Arc<dyn ShellExecApprovalGate> =
                        Arc::new(ConnectionApprovalGate::new(
                            id.clone(),
                            Arc::clone(&writer),
                            Arc::clone(&lines),
                            Some(Arc::clone(&sink)),
                            Some(Arc::clone(&cancel)),
                        ));
                    cancellation = Some(cancel);
                    gate = Some(approval_gate);
                    events = Some(sink);
                }
                handler
                    .handle_with_events(req, gate, events, cancellation)
                    .await
            }
            Err(e) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                format!("invalid JSON request: {e}"),
            ),
        };

        write_response_line(&writer, &response).await?;
    }

    Ok(())
}

struct ConnectionEventSink {
    writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
}

#[async_trait]
impl TurnEventSink for ConnectionEventSink {
    async fn progress(&self, id: &str, phase: ProgressPhase, message: Option<String>) {
        let response = ClientResponse::Progress {
            id: id.to_string(),
            phase,
            message,
        };
        let _ = write_response_line(&self.writer, &response).await;
    }

    async fn assistant_streaming(&self, id: &str, delta: String) {
        let response = ClientResponse::AssistantStreaming {
            id: id.to_string(),
            delta,
        };
        let _ = write_response_line(&self.writer, &response).await;
    }

    async fn final_response(&self, _id: &str) {}
}

async fn write_response_line(
    writer: &Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
    response: &ClientResponse,
) -> anyhow::Result<()> {
    use std::io::ErrorKind;

    let out = serde_json::to_string(response)? + "\n";
    let mut w = writer.lock().await;
    if let Err(e) = w.write_all(out.as_bytes()).await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    if let Err(e) = w.flush().await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    Ok(())
}
