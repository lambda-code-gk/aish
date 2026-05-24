//! LLM プロファイル選択の socket 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::ports::outbound::{ProfileRegistry, TerminationCapability, ToolsConfig};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn unknown_llm_profile_returns_invalid_request() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("profiles.sock");

    let registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        TerminationCapability::summary_prompt_only(),
    );
    let socket_for_server = socket_path.clone();
    let server = tokio::spawn(async move {
        server::run(socket_for_server, registry, ToolsConfig::default())
            .await
            .expect("server");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let payload = r#"{"type":"agent_turn","id":"x","llm_profile":"no-such","messages":[{"role":"user","content":"hi"}]}"#;
    let line = format!("{payload}\n");
    writer.write_all(line.as_bytes()).await.expect("write");
    writer.flush().await.expect("flush");

    let line = lines.next_line().await.expect("read").expect("line");
    assert!(line.contains(r#""type":"error""#));
    assert!(line.contains("invalid_request"));
    assert!(line.contains("no-such"));

    server.abort();
    let _ = server.await;
}
