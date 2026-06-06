//! `shell_exec` 実行前承認の Unix socket 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::ScriptedMockLlm;
use aibe::application::server;
use aibe::domain::{LlmStepResult, ToolCall, SHELL_EXEC};
use aibe::ports::outbound::{
    ProfileRegistry, ShellExecApprovalMode, ShellExecConfig, TerminationCapability, ToolsConfig,
};
use aibe_protocol::ClientResponse;
use serde_json::json;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn shell_exec_approval_denied_over_socket_continues_turn() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("approval.sock");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_exec".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "echo", "args": ["hi"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("user denied shell_exec"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["echo".into()],
        approval: ShellExecApprovalMode::Ask,
    };

    let socket_for_server = socket_path.clone();
    let cfg = tools_cfg.clone();
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let server = tokio::spawn(async move {
        server::run(socket_for_server, profile_registry, cfg, Vec::new())
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
        "id": "turn-approval",
        "messages": [{"role": "user", "content": "run echo"}],
        "tools": ["shell_exec"],
        "context": {"cwd": cwd.to_string_lossy()}
    });
    write_line(&mut writer, &req.to_string()).await;

    let prompt_line = read_line(&mut lines).await;
    let prompt: ClientResponse = serde_json::from_str(prompt_line.trim()).expect("prompt json");
    let ClientResponse::ShellExecApprovalPrompt {
        id,
        turn_id,
        tool_call_id,
        command,
        ..
    } = prompt
    else {
        panic!("expected shell_exec_approval_prompt, got {prompt_line}");
    };
    assert_eq!(turn_id, "turn-approval");
    assert_eq!(tool_call_id, "call_exec");
    assert_eq!(command, "echo");

    let denial = serde_json::json!({
        "type": "shell_exec_approval",
        "id": id,
        "turn_id": turn_id,
        "tool_call_id": tool_call_id,
        "approved": false
    });
    write_line(&mut writer, &denial.to_string()).await;

    let result_line = read_line(&mut lines).await;
    assert!(result_line.contains(r#""type":"agent_turn_result""#));
    assert!(result_line.contains("user denied shell_exec"));
    assert!(result_line.contains(r#""decision":"rejected_by_user""#));
    assert!(result_line.contains(r#""error":"approval_denied""#));

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
