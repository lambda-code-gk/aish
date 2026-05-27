//! aibe Unix socket クライアント（`ping` / `ensure_running`）。

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use aibe_protocol::{ClientRequest, ClientResponse};

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
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    let req = serde_json::to_string(&ClientRequest::Ping {
        id: "health".to_string(),
    })
    .expect("serialize ping");
    writeln!(stream, "{req}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
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
