//! Unix socket サーバの起動と接続ループ。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::application::request_service::RequestService;
use crate::ports::outbound::LlmProvider;
use crate::protocol::{ClientRequest, ClientResponse, ErrorCode};

pub async fn run(socket_path: PathBuf, llm: Arc<dyn LlmProvider>) -> anyhow::Result<()> {
    prepare_socket_path(&socket_path)?;
    let listener = bind_unix_listener(&socket_path)?;
    eprintln!("aibe: listening on {}", socket_path.display());

    let handler = Arc::new(RequestService::new(llm));

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

        let out = serde_json::to_string(&response)? + "\n";
        writer.write_all(out.as_bytes()).await?;
        writer.flush().await?;
    }

    Ok(())
}
