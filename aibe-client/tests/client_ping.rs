#![cfg(unix)]

mod common;

use aibe_client::ping;
use serial_test::serial;

#[test]
#[serial]
fn ping_detects_running_server() {
    let daemon = common::MockAibeDaemon::start();
    assert!(ping(&daemon.socket_path));
}
