//! write-like tool 承認の Unix socket 統合テスト（0054 Phase 5）。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::inbound::connection_approval::ConnectionApprovalGate;
use aibe::ports::outbound::{
    ToolApprovalGate, ToolApprovalGateOutcome, ToolApprovalPromptRequest, TurnCancellation,
};
use aibe_protocol::{ClientResponse, ToolApprovalOrigin};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

#[tokio::test]
async fn tool_approval_wire_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("tool-approval.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind");

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let lines = Arc::new(Mutex::new(BufReader::new(reader).lines()));
        let cancel = Arc::new(TurnCancellation::new());
        let gate = Arc::new(ConnectionApprovalGate::new(
            "turn-tool-approval".into(),
            Arc::clone(&writer),
            Arc::clone(&lines),
            None,
            Some(Arc::clone(&cancel)),
        ));
        let tool_gate: Arc<dyn ToolApprovalGate> = gate;
        let outcome = tool_gate
            .request_tool_approval(
                "call-write",
                ToolApprovalPromptRequest {
                    tool_name: "write_file".into(),
                    summary: "create demo.txt (+1 -0, 0 -> 5 bytes)".into(),
                    paths: vec!["demo.txt".into()],
                    preview: "+hello\n".into(),
                    preview_truncated: false,
                },
            )
            .await;
        assert_eq!(
            outcome,
            ToolApprovalGateOutcome::Approved(ToolApprovalOrigin::UiYes)
        );
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let prompt_line = read_until_tool_approval_prompt(&mut lines).await;
    let prompt: ClientResponse = serde_json::from_str(prompt_line.trim()).expect("prompt json");
    let ClientResponse::ToolApprovalPrompt {
        id,
        turn_id,
        tool_call_id,
        tool_name,
        preview,
        ..
    } = prompt
    else {
        panic!("expected tool_approval_prompt, got {prompt_line}");
    };
    assert_eq!(turn_id, "turn-tool-approval");
    assert_eq!(tool_call_id, "call-write");
    assert_eq!(tool_name, "write_file");
    assert_eq!(preview, "+hello\n");

    let approval = serde_json::json!({
        "type": "tool_approval",
        "id": id,
        "turn_id": turn_id,
        "tool_call_id": tool_call_id,
        "approved": true,
        "approval_origin": "ui_yes"
    });
    write_line(&mut writer, &approval.to_string()).await;

    server.await.expect("server");
}

async fn read_until_tool_approval_prompt(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    loop {
        let line = lines.next_line().await.expect("read line").expect("line");
        if line.contains(r#""type":"tool_approval_prompt""#) {
            return line;
        }
    }
}

async fn write_line(writer: &mut tokio::net::unix::OwnedWriteHalf, line: &str) {
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write");
    writer.flush().await.expect("flush");
}
