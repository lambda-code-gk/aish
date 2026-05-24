#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::client;
use aibe::ports::outbound::{TerminationCapability, ToolsConfig};
use tempfile::tempdir;
use tokio::runtime::Runtime;

#[test]
fn ping_detects_running_server() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("ping.sock");
    let socket_for_server = socket_path.clone();

    std::thread::spawn(move || {
        let rt = Runtime::new().expect("runtime");
        rt.block_on(async {
            server::run(
                socket_for_server,
                Arc::new(MockLlm::new()),
                ToolsConfig::default(),
                TerminationCapability::summary_prompt_only(),
            )
            .await
            .expect("server");
        });
    });

    std::thread::sleep(Duration::from_millis(80));
    assert!(client::ping(&socket_path));
}
