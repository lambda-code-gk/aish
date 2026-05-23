//! `AgentTurnService` のユースケース単体テスト（integration クレート配置で adapters 利用可）。

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::ScriptedMockLlm;
use aibe::application::agent_turn::AgentTurnService;
use aibe::domain::{ChatMessage, LlmStepResult, ToolCall};
use aibe::ports::outbound::{LlmError, LlmProvider, ShellExecConfig, ToolDefinition, ToolsConfig};
use aibe::protocol::{ClientResponse, ErrorCode, RequestContext};
use async_trait::async_trait;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn empty_tools_uses_complete_path() {
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only("done")]));
    let cfg = ToolsConfig::default();
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("hi")],
            vec![],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => {
            assert_eq!(assistant_message.content, "done");
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

#[tokio::test]
async fn unknown_tool_in_request_is_rejected() {
    let llm = Arc::new(ScriptedMockLlm::new(vec![]));
    let cfg = ToolsConfig::default();
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("hi")],
            vec!["nope".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::ToolNotAllowed),
        _ => panic!("expected error"),
    }
}

#[tokio::test]
async fn shell_exec_not_allowed_returns_tool_result_and_continues() {
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "curl", "args": []}),
            }],
        ),
        LlmStepResult::text_only("gave up on curl"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["echo".into()],
    };
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("fetch")],
            vec!["shell_exec".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "gave up on curl");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "error");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn max_tool_rounds_returns_agent_turn_result_with_tool_calls() {
    let dir = tempdir().expect("tempdir");
    let file_path = dir.path().join("README.txt");
    fs::write(&file_path, "line one\nline two\n").expect("write");

    let read_call = |id: &str| ToolCall {
        id: id.into(),
        name: "read_file".into(),
        arguments: json!({"path": file_path}),
    };
    let steps = vec![
        LlmStepResult::with_tool_calls("", vec![read_call("c1")]),
        LlmStepResult::with_tool_calls("", vec![read_call("c2")]),
        LlmStepResult::text_only("Tool round limit reached. Summary from reads above."),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.max_rounds = 2;
    cfg.read_file.allowed_roots = vec![dir.path().to_path_buf()];
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "max-rounds".into(),
            vec![ChatMessage::user("read all")],
            vec!["read_file".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            status,
            tool_calls,
            assistant_message,
            ..
        } => {
            assert_eq!(status, "max_tool_rounds");
            assert_eq!(tool_calls.len(), 2);
            assert!(assistant_message.content.contains("Tool round limit"));
        }
        ClientResponse::Error { code, message, .. } => {
            panic!("expected agent_turn_result, got error {code:?}: {message}");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn shell_exec_output_is_truncated_for_llm_and_tool_calls() {
    let payload = "x".repeat(500);
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "echo", "args": [payload]}),
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.max_tool_output_bytes = 80;
    cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["echo".into()],
    };
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("run")],
            vec!["shell_exec".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            let output = tool_calls[0]["output"].as_str().expect("output string");
            assert!(output.contains("[output truncated:"));
            assert!(output.len() < 500);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn read_file_output_is_truncated_for_llm_and_tool_calls() {
    let dir = tempdir().expect("tempdir");
    let file_path = dir.path().join("large.txt");
    fs::write(&file_path, "z".repeat(500)).expect("write");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: json!({"path": file_path}),
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let inner = ScriptedMockLlm::new(steps);
    let llm = Arc::new(TruncationAssertLlm {
        inner,
        round: AtomicUsize::new(0),
    });
    let mut cfg = ToolsConfig::default();
    cfg.max_tool_output_bytes = 300;
    cfg.read_file.allowed_roots = vec![dir.path().to_path_buf()];
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("read")],
            vec!["read_file".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            let output = tool_calls[0]["output"].as_str().expect("output string");
            assert!(output.contains("[output truncated:"));
            assert!(output.len() < 500);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn model_disallowed_tool_returns_tool_result_and_continues() {
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "delete_everything".into(),
                arguments: json!({}),
            }],
        ),
        LlmStepResult::text_only("cannot delete, explained"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let cfg = ToolsConfig::default();
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("clean disk")],
            vec!["read_file".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "cannot delete, explained");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "error");
            assert_eq!(tool_calls[0]["error"], "tool_not_allowed");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn read_file_outside_allowed_roots_returns_tool_result_and_continues() {
    let dir = tempdir().expect("tempdir");
    let outside = dir.path().join("secret.txt");
    fs::write(&outside, "secret").expect("write");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: json!({"path": outside}),
            }],
        ),
        LlmStepResult::text_only("cannot read that path"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    let allowed = dir.path().join("allowed");
    fs::create_dir(&allowed).expect("mkdir");
    cfg.read_file.allowed_roots = vec![allowed];
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("read secret")],
            vec!["read_file".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "cannot read that path");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "error");
            assert_eq!(tool_calls[0]["error"], "path_not_allowed");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn shell_exec_nonzero_exit_returns_tool_result_and_continues() {
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "false", "args": []}),
            }],
        ),
        LlmStepResult::text_only("command failed as expected"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["false".into()],
    };
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("run check")],
            vec!["shell_exec".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "command failed as expected");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "error");
            assert_eq!(tool_calls[0]["error"], "nonzero_exit");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn shell_exec_timeout_returns_tool_result_and_continues() {
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "sleep", "args": ["5"]}),
            }],
        ),
        LlmStepResult::text_only("sleep timed out"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.exec_timeout_ms = 100;
    cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["sleep".into()],
    };
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "1".into(),
            vec![ChatMessage::user("nap")],
            vec!["shell_exec".into()],
            RequestContext::default(),
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "sleep timed out");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "error");
            assert_eq!(tool_calls[0]["error"], "timeout");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn shell_exec_runs_in_context_cwd() {
    let dir = tempdir().expect("tempdir");
    let client_sub = dir.path().join("work");
    fs::create_dir_all(&client_sub).expect("mkdir");
    fs::write(client_sub.join("note.txt"), "shell cwd ok").expect("write");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "cat", "args": ["note.txt"]}),
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.shell_exec = ShellExecConfig {
        enabled: true,
        allowed_commands: vec!["cat".into()],
    };
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "cwd-shell".into(),
            vec![ChatMessage::user("run")],
            vec!["shell_exec".into()],
            RequestContext {
                cwd: Some(client_sub.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "ok");
            let output = tool_calls[0]["output"].as_str().unwrap_or("");
            assert!(output.contains("shell cwd ok"), "output: {output}");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn read_file_relative_path_uses_context_cwd() {
    let dir = tempdir().expect("tempdir");
    let client_sub = dir.path().join("work");
    fs::create_dir_all(&client_sub).expect("mkdir");
    fs::write(client_sub.join("rel.txt"), "relative ok").expect("write");

    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: json!({"path": "rel.txt"}),
            }],
        ),
        LlmStepResult::text_only("read rel.txt"),
    ];
    let llm = Arc::new(ScriptedMockLlm::new(steps));
    let mut cfg = ToolsConfig::default();
    cfg.read_file.allowed_roots = vec![dir.path().to_path_buf()];
    let svc = AgentTurnService::new(llm, build_registry(&cfg), cfg);
    let res = svc
        .run(
            "cwd".into(),
            vec![ChatMessage::user("read")],
            vec!["read_file".into()],
            RequestContext {
                cwd: Some(client_sub.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0]["status"], "ok");
            assert_eq!(tool_calls[0]["output"], "relative ok");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

/// 2 回目の LLM 呼び出し前に、会話中の tool result が切り詰められていることを検証する。
struct TruncationAssertLlm {
    inner: ScriptedMockLlm,
    round: AtomicUsize,
}

#[async_trait]
impl LlmProvider for TruncationAssertLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        self.inner.complete(messages).await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        if round == 1 {
            let tool_msg = messages
                .iter()
                .find(|m| m.role == "tool")
                .expect("tool result in conversation");
            assert!(
                tool_msg.content.contains("[output truncated:"),
                "LLM tool result must be truncated (len={})",
                tool_msg.content.len()
            );
            assert!(tool_msg.content.len() < 500);
        }
        self.inner.complete_with_tools(messages, tools).await
    }
}
