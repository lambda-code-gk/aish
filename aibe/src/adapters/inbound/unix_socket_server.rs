//! Unix domain socket の bind / accept / NDJSON フレーミング。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::{sleep_until, Instant};

use crate::adapters::inbound::client_tool_gate::ConnectionClientToolGate;
use crate::adapters::inbound::connection_approval::ConnectionApprovalGate;
use crate::adapters::inbound::connection_human_task::ConnectionHumanTaskGate;
use crate::ports::inbound::ClientRequestHandler;
use crate::ports::inbound::ShutdownCoordinator;
use crate::ports::outbound::{
    ClientToolGate, HumanTaskGate, ShellExecApprovalGate, ToolApprovalGate, TurnCancellation,
    TurnEventSink,
};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, ProgressPhase};

pub const DEFAULT_SHUTDOWN_DRAIN: Duration = Duration::from_secs(5);

pub async fn run(
    socket_path: PathBuf,
    handler: Arc<dyn ClientRequestHandler>,
    shutdown: Arc<ShutdownCoordinator>,
) -> anyhow::Result<()> {
    prepare_socket_path(&socket_path)?;
    let listener = bind_unix_listener(&socket_path)?;
    eprintln!("aibe: listening on {}", socket_path.display());

    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, _) = accept?;
                let handler = Arc::clone(&handler);
                let shutdown = Arc::clone(&shutdown);
                connections.spawn(async move {
                    if let Err(e) = serve_connection(stream, handler, shutdown).await {
                        eprintln!("aibe: connection error: {e}");
                    }
                });
            }
            () = shutdown.wait() => {
                break;
            }
        }
    }

    drop(listener);
    drain_connections(&mut connections, DEFAULT_SHUTDOWN_DRAIN).await;
    Ok(())
}

async fn drain_connections(connections: &mut JoinSet<()>, timeout: Duration) {
    if connections.is_empty() {
        return;
    }
    let deadline = Instant::now() + timeout;
    loop {
        tokio::select! {
            next = connections.join_next() => {
                match next {
                    Some(Ok(())) | Some(Err(_)) => {
                        if connections.is_empty() {
                            return;
                        }
                    }
                    None => return,
                }
            }
            () = sleep_until(deadline) => {
                connections.abort_all();
                while connections.join_next().await.is_some() {}
                return;
            }
        }
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
    shutdown: Arc<ShutdownCoordinator>,
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
                        .handle_memory_subscribe(
                            body,
                            Arc::clone(&writer),
                            Arc::clone(&lines),
                            Some(Arc::clone(&shutdown)),
                        )
                        .await?;
                    break;
                }
                let mut cancellation: Option<Arc<TurnCancellation>> = None;
                let mut gate: Option<Arc<dyn ShellExecApprovalGate>> = None;
                let mut tool_gate: Option<Arc<dyn ToolApprovalGate>> = None;
                let mut client_tool_gate: Option<Arc<dyn ClientToolGate>> = None;
                let mut human_task_gate: Option<Arc<dyn HumanTaskGate>> = None;
                let mut events: Option<Arc<dyn TurnEventSink>> = None;
                if let ClientRequest::AgentTurn { id, .. } = &req {
                    let cancel = Arc::new(TurnCancellation::new());
                    let sink: Arc<dyn TurnEventSink> = Arc::new(ConnectionEventSink {
                        writer: Arc::clone(&writer),
                    });
                    let connection_gate = Arc::new(ConnectionApprovalGate::new(
                        id.clone(),
                        Arc::clone(&writer),
                        Arc::clone(&lines),
                        Some(Arc::clone(&sink)),
                        Some(Arc::clone(&cancel)),
                    ));
                    let tool_approval_gate: Arc<dyn ToolApprovalGate> = connection_gate.clone();
                    let approval_gate: Arc<dyn ShellExecApprovalGate> = connection_gate;
                    let tool_gate_impl: Arc<dyn ClientToolGate> =
                        Arc::new(ConnectionClientToolGate::new(
                            id.clone(),
                            Arc::clone(&writer),
                            Arc::clone(&lines),
                            Some(Arc::clone(&sink)),
                            Some(Arc::clone(&cancel)),
                        ));
                    let human_gate_impl: Arc<dyn HumanTaskGate> =
                        Arc::new(ConnectionHumanTaskGate::new(
                            id.clone(),
                            Arc::clone(&writer),
                            Arc::clone(&lines),
                            Some(Arc::clone(&sink)),
                            Some(Arc::clone(&cancel)),
                        ));
                    cancellation = Some(cancel);
                    gate = Some(approval_gate);
                    tool_gate = Some(tool_approval_gate);
                    client_tool_gate = Some(tool_gate_impl);
                    human_task_gate = Some(human_gate_impl);
                    events = Some(sink);
                }
                handler
                    .handle_with_events(
                        req,
                        gate,
                        tool_gate,
                        client_tool_gate,
                        human_task_gate,
                        events,
                        cancellation,
                    )
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
