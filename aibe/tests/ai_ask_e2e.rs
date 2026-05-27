#![cfg(unix)]

//! `ai ask` が mock aibe へ socket 経由で接続できること（0017: server E2E は aibe 側）。

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ai::adapters::outbound::{AibeUnixClient, StdoutPresenter};
use ai::application::{Ask, AskRunOptions};
use ai::domain::{resolve_tools, ConfigToolsTokens};
use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::ports::outbound::{ProfileRegistry, TerminationCapability, ToolsConfig};
use tempfile::tempdir;
use tokio::runtime::Runtime;

#[test]
fn ai_ask_reaches_mock_aibe() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("ai-ask-e2e.sock");

    let socket_for_server = socket_path.clone();
    thread::spawn(move || {
        let rt = Runtime::new().expect("runtime");
        rt.block_on(async {
            let registry = ProfileRegistry::single(
                "default",
                Arc::new(MockLlm::new()),
                TerminationCapability::summary_prompt_only(),
            );
            server::run(socket_for_server, registry, ToolsConfig::default())
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
    let resolved = resolve_tools(None, &ConfigToolsTokens::default()).expect("resolve");
    ask.run(
        "integration test".to_string(),
        AskRunOptions {
            resolved_tools: resolved,
            verbose_tools: false,
            llm_profile: None,
        },
    )
    .expect("ask");
}
