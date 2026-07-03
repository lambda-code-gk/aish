//! aibe Unix socket クライアント（transport / ping / ensure_running）。

#![cfg(unix)]

mod transport;
mod unix_connect;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use aibe_protocol::{ClientRequest, ClientResponse};

pub use transport::{
    agent_turn, agent_turn_on_stream, agent_turn_on_stream_with_callbacks,
    agent_turn_with_client_tools, agent_turn_with_client_tools_on_stream, agent_turn_with_events,
    agent_turn_with_events_on_stream, memory_request, memory_request_on_stream, read_response_line,
    route_turn, route_turn_on_stream, send_cancel_request, send_request, shell_exec_only_callbacks,
    AgentTurnCallbacks, AgentTurnProgressEvent, ClientError, ClientToolCallRequest,
    ShellExecApprovalDecision, ShellExecApprovalPrompt, ToolApprovalDecision, ToolApprovalPrompt,
};

use unix_connect::connect_unix_stream;

/// `ping` の connect / read / write 上限。
const PING_IO_TIMEOUT: Duration = Duration::from_millis(500);

/// デフォルトの Unix socket パス（`$HOME/.local/share/aibe/run.sock`）。
pub fn default_socket_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local/share/aibe")
        .join("run.sock")
}

/// 既存デーモンへ `ping` し、`pong` なら true。
pub fn ping(socket_path: &Path) -> bool {
    ping_result(socket_path).unwrap_or(false)
}

pub fn ping_result(socket_path: &Path) -> std::io::Result<bool> {
    let mut stream = match connect_unix_stream(socket_path, PING_IO_TIMEOUT) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    let _ = stream.set_read_timeout(Some(PING_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PING_IO_TIMEOUT));
    let req = ClientRequest::Ping {
        id: "health".to_string(),
    };
    send_request(&mut stream, &req)?;
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let Ok(ClientResponse::Pong { .. }) = serde_json::from_str::<ClientResponse>(line.trim())
    else {
        return Ok(false);
    };
    Ok(true)
}

/// `ping` の詳細版。接続エラーはそのまま返し、`pong` 応答なら true。
pub fn ping_detailed(socket_path: &Path) -> std::io::Result<bool> {
    let mut stream = connect_unix_stream(socket_path, PING_IO_TIMEOUT)?;
    let _ = stream.set_read_timeout(Some(PING_IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PING_IO_TIMEOUT));
    let req = ClientRequest::Ping {
        id: "health".to_string(),
    };
    send_request(&mut stream, &req)?;
    let mut reader = BufReader::new(&mut stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let Ok(ClientResponse::Pong { .. }) = serde_json::from_str::<ClientResponse>(line.trim())
    else {
        return Ok(false);
    };
    Ok(true)
}

/// 応答がなければ `aibe` バイナリを起動し、最大約 5 秒待つ。
pub fn ensure_running(socket_path: &Path) -> Result<(), String> {
    if ping(socket_path) {
        return Ok(());
    }

    let bin = resolve_aibe_binary();
    Command::new(&bin)
        .env("AIBE_SOCKET_PATH", socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if ping(socket_path) {
            return Ok(());
        }
    }
    Err(format!(
        "aibe did not become ready at {}",
        socket_path.display()
    ))
}

fn resolve_aibe_binary() -> PathBuf {
    if let Ok(p) = std::env::var("AIBE_BIN") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("aibe");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("aibe")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn ping_missing_socket_returns_false_within_deadline() {
        let path =
            std::env::temp_dir().join(format!("aibe-client-missing-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let start = Instant::now();
        assert!(!ping(&path));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "ping blocked for {:?}",
            start.elapsed()
        );
    }
}
