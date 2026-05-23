#![cfg(unix)]

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ai::adapters::outbound::{AibeUnixClient, StdoutPresenter};
use ai::application::Ask;
use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::ports::outbound::ToolsConfig;
use tempfile::tempdir;
use tokio::runtime::Runtime;

#[test]
fn ask_reaches_mock_aibe() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("ai-test.sock");

    let socket_for_server = socket_path.clone();
    thread::spawn(move || {
        let rt = Runtime::new().expect("runtime");
        rt.block_on(async {
            server::run(
                socket_for_server,
                Arc::new(MockLlm::new()),
                ToolsConfig::default(),
            )
            .await
            .expect("server");
        });
    });

    thread::sleep(Duration::from_millis(80));

    let client = AibeUnixClient::new(&socket_path);
    let presenter = StdoutPresenter;
    let ask = Ask::new(
        &client,
        &presenter,
        None::<&ai::adapters::outbound::FileLogTail>,
    );
    ask.run("integration test".to_string()).expect("ask");
}
