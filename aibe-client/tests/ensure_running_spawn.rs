#![cfg(unix)]

//! `ensure_running` が `AIBE_BIN` / `AIBE_SOCKET_PATH` で aibe を起動すること。

mod common;

use std::time::Duration;

use aibe_client::{ensure_running, ping};
use serial_test::serial;
use tempfile::tempdir;

#[test]
#[serial]
fn ensure_running_spawns_aibe_at_custom_socket() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("spawn.sock");
    assert!(!ping(&socket_path));

    let bin = common::aibe_binary();
    assert!(bin.is_file(), "aibe binary not found at {}", bin.display());
    std::env::set_var("AIBE_BIN", &bin);

    ensure_running(&socket_path).expect("ensure_running");
    assert!(ping(&socket_path));

    std::thread::sleep(Duration::from_millis(50));
}
