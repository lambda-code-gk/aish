//! Unix socket サーバの起動と接続ループ。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use crate::adapters::outbound::tools::build_registry;
use crate::application::request_service::RequestService;
use crate::application::tool_round::ToolRoundExecutor;
use crate::ports::outbound::{LlmProvider, TerminationCapability, ToolsConfig};
use crate::protocol::{ClientRequest, ClientResponse, ErrorCode};

pub async fn run(
    socket_path: PathBuf,
    llm: Arc<dyn LlmProvider>,
    tools_config: ToolsConfig,
    termination_capability: TerminationCapability,
) -> anyhow::Result<()> {
    prepare_socket_path(&socket_path)?;
    let listener = bind_unix_listener(&socket_path)?;
    eprintln!("aibe: listening on {}", socket_path.display());

    let registry = build_registry(&tools_config);
    let executor = ToolRoundExecutor::new(
        Arc::clone(&llm),
        Arc::clone(&registry),
        tools_config.clone(),
    );
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        tools_config.termination_strategy,
    ));
    let handler = Arc::new(RequestService::new(
        llm,
        executor,
        terminator,
        termination_capability,
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

/// `bind` 前後で umask を 077 にし、作成直後から他ユーザーに開けないようにする。
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
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<ClientRequest>(line) {
            Ok(req) => handler.handle(req).await,
            Err(e) => ClientResponse::error(
                String::new(),
                ErrorCode::InvalidRequest,
                format!("invalid JSON request: {e}"),
            ),
        };

        write_response_line(&mut writer, &response).await?;
    }

    Ok(())
}

/// 応答 1 行を書き込む。クライアントが先に切断した場合は正常終了（手動 `socat` 等）。
async fn write_response_line(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response: &ClientResponse,
) -> anyhow::Result<()> {
    use std::io::ErrorKind;

    let out = serde_json::to_string(response)? + "\n";
    if let Err(e) = writer.write_all(out.as_bytes()).await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    if let Err(e) = writer.flush().await {
        if e.kind() == ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(e.into());
    }
    Ok(())
}
