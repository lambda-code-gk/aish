#![cfg(unix)]

//! `ensure_running` が既に起動済みの socket で即成功すること。

mod common;

use aibe_client::{ensure_running, ping};

#[test]
fn ensure_running_waits_on_custom_socket_path() {
    let daemon = common::MockAibeDaemon::start();
    assert!(ping(&daemon.socket_path));
    ensure_running(&daemon.socket_path).expect("ensure on running daemon");
}
