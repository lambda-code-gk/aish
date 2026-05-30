//! Unix socket サーバの起動と接続ループ。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::adapters::inbound::connection_approval::ConnectionApprovalGate;
use crate::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use crate::adapters::outbound::tools::build_registry;
use crate::application::request_service::RequestService;
use crate::ports::outbound::{ProfileRegistry, ShellExecApprovalGate, ToolsConfig};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode};

pub async fn run(
    socket_path: PathBuf,
    profile_registry: ProfileRegistry,
    tools_config: ToolsConfig,
) -> anyhow::Result<()> {
    prepare_socket_path(&socket_path)?;
    let listener = bind_unix_listener(&socket_path)?;
    eprintln!("aibe: listening on {}", socket_path.display());

    let tool_registry = build_registry(&tools_config);
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        tools_config.termination_strategy,
    ));
    let handler = Arc::new(RequestService::new(
        profile_registry,
        tool_registry,
        tools_config,
        terminator,
    ));

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

async fn serve_connection(stream: UnixStream, handler: Arc<RequestService>) -> anyhow::Result<()> {
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
                let gate = match &req {
                    ClientRequest::AgentTurn { id, .. } => {
                        let gate: Arc<dyn ShellExecApprovalGate> =
                            Arc::new(ConnectionApprovalGate::new(
                                id.clone(),
                                Arc::clone(&writer),
                                Arc::clone(&lines),
                            ));
                        Some(gate)
                    }
                    _ => None,
                };
                handler.handle(req, gate).await
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
