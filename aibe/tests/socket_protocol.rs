//! Unix socket 経由の ping / agent_turn 統合テスト。

#![cfg(unix)]

use std::time::Duration;

use std::sync::Arc;

use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::ports::outbound::{TerminationCapability, ToolsConfig};
use aibe::protocol::{ClientRequest, ClientResponse};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn ping_and_agent_turn_over_unix_socket() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("test.sock");

    let socket_for_server = socket_path.clone();
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            Arc::new(MockLlm::new()),
            ToolsConfig::default(),
            TerminationCapability::summary_prompt_only(),
        )
        .await
        .expect("server");
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    write_line(&mut writer, r#"{"type":"ping","id":"p1"}"#).await;
    let pong = read_line(&mut lines).await;
    assert!(pong.contains(r#""type":"pong""#));
    assert!(pong.contains(r#""id":"p1""#));

    write_line(
        &mut writer,
        r#"{"type":"agent_turn","id":"t1","messages":[{"role":"user","content":"hello"}]}"#,
    )
    .await;
    let result = read_line(&mut lines).await;
    assert!(result.contains(r#""type":"agent_turn_result""#));
    assert!(result.contains(r#"[mock] received: hello"#));

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

#[test]
fn protocol_roundtrip() {
    let req: ClientRequest = serde_json::from_str(r#"{"type":"ping","id":"x"}"#).expect("parse");
    assert!(matches!(req, ClientRequest::Ping { .. }));

    let res = ClientResponse::Pong {
        id: "x".to_string(),
    };
    let json = serde_json::to_string(&res).expect("serialize");
    assert!(json.contains("pong"));
}
