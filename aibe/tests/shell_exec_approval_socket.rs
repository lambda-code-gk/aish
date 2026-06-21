//! `shell_exec` 実行前承認の Unix socket 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::ScriptedMockLlm;
use aibe::application::server;
use aibe::domain::{LlmStepResult, ToolCall, SHELL_EXEC};
use aibe::ports::outbound::{
    MemoryConfig, ProfileRegistry, ShellExecApprovalMode, ShellExecConfig, TerminationCapability,
    ToolsConfig,
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
        ..Default::default()
    };

    let socket_for_server = socket_path.clone();
    let cfg = tools_cfg.clone();
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            dir.path().join("test-config.toml"),
            profile_registry,
            cfg,
            Vec::new(),
            "default".to_string(),
            dir.path().join("conversations"),
            MemoryConfig::default(),
        )
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

    let prompt_line = read_until_shell_exec_prompt(&mut lines).await;
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
        "approved": false,
        "approval_origin": "ui_no"
    });
    write_line(&mut writer, &denial.to_string()).await;

    let result_line = read_until_agent_turn_result(&mut lines).await;
    assert!(result_line.contains(r#""type":"agent_turn_result""#));
    assert!(result_line.contains("user denied shell_exec"));
    assert!(result_line.contains(r#""decision":"rejected_by_user""#));
    assert!(result_line.contains(r#""error":"approval_denied""#));
    assert!(result_line.contains(r#""approval_source":"shell_exec_approval=ask;ui=n""#));

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn shell_exec_session_allowed_audit_over_socket() {
    run_approval_audit_case(
        "session_allowed",
        "echo",
        json!({"command": "echo", "args": ["hi"]}),
        true,
        "session_allowed",
        "auto_approved_session",
        "shell_exec_approval=ask;cache=session",
    )
    .await;
}

#[tokio::test]
async fn shell_exec_pattern_read_only_audit_over_socket() {
    run_approval_audit_case(
        "pattern_read_only",
        "echo",
        json!({"command": "echo", "args": ["hi"]}),
        true,
        "pattern_read_only",
        "auto_approved_pattern",
        "shell_exec_approval=ask;pattern=read_only",
    )
    .await;
}

async fn run_approval_audit_case(
    turn_id: &str,
    allowed_command: &str,
    tool_args: serde_json::Value,
    approved: bool,
    approval_origin: &str,
    expected_decision: &str,
    expected_approval_source: &str,
) {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("approval.sock");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_exec".into(),
                name: SHELL_EXEC.to_string(),
                arguments: tool_args,
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec![allowed_command.into()],
        approval: ShellExecApprovalMode::Ask,
        ..Default::default()
    };

    let socket_for_server = socket_path.clone();
    let cfg = tools_cfg.clone();
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            dir.path().join("test-config.toml"),
            profile_registry,
            cfg,
            Vec::new(),
            "default".to_string(),
            dir.path().join("conversations"),
            MemoryConfig::default(),
        )
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
        "id": turn_id,
        "messages": [{"role": "user", "content": "run shell"}],
        "tools": ["shell_exec"],
        "context": {"cwd": cwd.to_string_lossy()}
    });
    write_line(&mut writer, &req.to_string()).await;

    let prompt_line = read_until_shell_exec_prompt(&mut lines).await;
    let prompt: ClientResponse = serde_json::from_str(prompt_line.trim()).expect("prompt json");
    let ClientResponse::ShellExecApprovalPrompt {
        id,
        turn_id: prompt_turn_id,
        tool_call_id,
        ..
    } = prompt
    else {
        panic!("expected shell_exec_approval_prompt, got {prompt_line}");
    };

    let approval = serde_json::json!({
        "type": "shell_exec_approval",
        "id": id,
        "turn_id": prompt_turn_id,
        "tool_call_id": tool_call_id,
        "approved": approved,
        "approval_origin": approval_origin
    });
    write_line(&mut writer, &approval.to_string()).await;

    let result_line = read_until_agent_turn_result(&mut lines).await;
    assert!(result_line.contains(r#""type":"agent_turn_result""#));
    assert!(
        result_line.contains(&format!(r#""decision":"{expected_decision}""#)),
        "result: {result_line}"
    );
    assert!(
        result_line.contains(&format!(
            r#""approval_source":"{expected_approval_source}""#
        )),
        "result: {result_line}"
    );

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

async fn read_until_shell_exec_prompt(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    loop {
        let line = read_line(lines).await;
        let response: serde_json::Value = serde_json::from_str(&line).expect("json");
        if response.get("type").and_then(|v| v.as_str()) == Some("shell_exec_approval_prompt") {
            return line;
        }
    }
}

async fn read_until_agent_turn_result(
    lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> String {
    loop {
        let line = read_line(lines).await;
        let response: serde_json::Value = serde_json::from_str(&line).expect("json");
        if response.get("type").and_then(|v| v.as_str()) == Some("agent_turn_result") {
            return line;
        }
    }
}
