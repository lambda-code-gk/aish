//! ツール付き `agent_turn` の Unix socket 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::ScriptedMockLlm;
use aibe::application::server;
use aibe::domain::{LlmStepResult, ToolCall};
use aibe::ports::outbound::ToolsConfig;
use serde_json::json;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn tool_loop_over_socket_returns_final_and_tool_calls() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("tools.sock");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "Cargo.toml", "limit": 3}),
            }],
        ),
        LlmStepResult::text_only("read done"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.read_file.allowed_roots = vec![std::env::current_dir().expect("cwd")];

    let socket_for_server = socket_path.clone();
    let cfg = tools_cfg.clone();
    let server = tokio::spawn(async move {
        server::run(socket_for_server, llm, cfg)
            .await
            .expect("server");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let cwd = std::env::current_dir().expect("cwd");
    let req = serde_json::json!({
        "type": "agent_turn",
        "id": "turn-tools",
        "messages": [{"role": "user", "content": "read manifest"}],
        "tools": ["read_file"],
        "context": {"cwd": cwd.to_string_lossy()}
    });
    write_line(&mut writer, &req.to_string()).await;
    let result = read_line(&mut lines).await;
    assert!(result.contains(r#""type":"agent_turn_result""#));
    assert!(result.contains("read done"));
    assert!(result.contains(r#""status":"ok""#));
    assert!(result.contains("read_file"));

    server.abort();
    let _ = server.await;
}

async fn write_line(writer: &mut tokio::net::unix::OwnedWriteHalf, json: &str) {
    let line = format!("{json}\n");
    writer.write_all(line.as_bytes()).await.expect("write");
    writer.flush().await.expect("flush");
}

async fn read_line(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    lines.next_line().await.expect("read").expect("line")
}
