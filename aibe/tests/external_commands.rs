//! 外部コマンド（`[[external_commands]]`）の `shell_exec` 統合テスト。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use aibe::adapters::outbound::ScriptedMockLlm;
use aibe::application::completion_envelope::MINIMAL_CONTRACT_BEFORE_TOOLS;
use aibe::application::server;
use aibe::domain::{LlmStepResult, ToolCall, SHELL_EXEC};
use aibe::ports::outbound::{
    ExternalCommandConfig, MemoryConfig, ProfileRegistry, ShellExecApprovalMode, ShellExecConfig,
    TerminationCapability, ToolsConfig,
};
use serde_json::json;
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn external_command_runs_via_shell_exec() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("external.sock");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            MINIMAL_CONTRACT_BEFORE_TOOLS,
            vec![ToolCall {
                id: "call_ext".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "echo", "args": ["external-ok"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let tools_cfg = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        },
        ..Default::default()
    };
    let external_commands = vec![ExternalCommandConfig {
        name: "fixture-echo".into(),
        description: "fixture".into(),
        command: "echo".into(),
        args: vec!["{prompt}".into()],
        timeout_secs: 30,
    }];

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
            external_commands,
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
    let req = json!({
        "type": "agent_turn",
        "id": "turn-1",
        "messages": [{"role": "user", "content": "run external"}],
        "tools": ["shell_exec"],
        "context": { "cwd": cwd.to_string_lossy() }
    });
    write_line(&mut writer, &req.to_string()).await;

    let mut saw_external_ok = false;
    let mut saw_approval_source = false;
    while let Some(line) = lines.next_line().await.expect("read") {
        let response: serde_json::Value = serde_json::from_str(&line).expect("json");
        if response.get("type").and_then(|v| v.as_str()) != Some("agent_turn_result") {
            continue;
        }
        let tool_calls = response
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .expect("tool_calls");
        for tc in tool_calls {
            if tc.get("name").and_then(|v| v.as_str()) == Some("shell_exec") {
                if tc
                    .get("approval_source")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.contains("external_command=fixture-echo"))
                {
                    saw_approval_source = true;
                }
                if tc
                    .get("output")
                    .and_then(|v| v.as_str())
                    .is_some_and(|r| r.contains("external-ok"))
                {
                    saw_external_ok = true;
                }
            }
        }
        break;
    }

    server.abort();
    assert!(saw_external_ok, "expected echo stdout in tool result");
    assert!(
        saw_approval_source,
        "expected external_command in approval_source"
    );
}

#[tokio::test]
async fn external_command_not_in_allowlist_is_denied() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("external-deny.sock");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            MINIMAL_CONTRACT_BEFORE_TOOLS,
            vec![ToolCall {
                id: "call_ext".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "echo", "args": ["nope"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("denied"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let tools_cfg = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands: vec![],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        },
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
    let req = json!({
        "type": "agent_turn",
        "id": "turn-2",
        "messages": [{"role": "user", "content": "run denied"}],
        "tools": ["shell_exec"],
        "context": { "cwd": cwd.to_string_lossy() }
    });
    write_line(&mut writer, &req.to_string()).await;

    let mut saw_denied = false;
    while let Some(line) = lines.next_line().await.expect("read") {
        let response: serde_json::Value = serde_json::from_str(&line).expect("json");
        if response.get("type").and_then(|v| v.as_str()) != Some("agent_turn_result") {
            continue;
        }
        let tool_calls = response
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .expect("tool_calls");
        for tc in tool_calls {
            if tc.get("error").and_then(|v| v.as_str()) == Some("command_not_allowed") {
                saw_denied = true;
            }
        }
        break;
    }

    server.abort();
    assert!(saw_denied, "expected command_not_allowed");
}

async fn write_line<W: AsyncWriteExt + Unpin>(writer: &mut W, line: &str) {
    writer
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write");
    writer.flush().await.expect("flush");
}
