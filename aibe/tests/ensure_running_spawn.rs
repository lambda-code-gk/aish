#![cfg(unix)]

//! `ensure_running` が `AIBE_SOCKET_PATH` を渡して aibe を起動すること。

use std::path::PathBuf;
use std::time::Duration;

use aibe::client;
use tempfile::tempdir;

fn test_aibe_binary() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_aibe") {
        return PathBuf::from(p);
    }
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../target/{profile}/aibe"))
}

#[test]
fn ensure_running_spawns_aibe_at_custom_socket() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("spawn.sock");
    assert!(!client::ping(&socket_path));

    let bin = test_aibe_binary();
    assert!(bin.is_file(), "aibe binary not found at {}", bin.display());
    std::env::set_var("AIBE_BIN", &bin);

    client::ensure_running(&socket_path).expect("ensure_running");
    assert!(client::ping(&socket_path));

    std::thread::sleep(Duration::from_millis(50));
}
