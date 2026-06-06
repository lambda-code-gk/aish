#![cfg(unix)]

use std::sync::{Arc, Mutex};

use ai::adapters::outbound::{render_response, StdoutPresenter};
use ai::application::{plan_ask_launch, Ask, AskRunOptions};
use ai::domain::{resolve_tools, ConfigToolsTokens, ToolsResolveError};
use ai::ports::outbound::{AgentClient, AgentError, Presenter};
use aibe_protocol::{
    AgentTurnStatus, ClientResponse, ExecutedToolCall, ProtocolMessageOut, ToolName,
    MAX_TOOL_OUTPUT_BYTES,
};
use serde_json::json;

struct RecordingClient {
    last_tools: Arc<Mutex<Vec<String>>>,
}

impl RecordingClient {
    fn new() -> Self {
        Self {
            last_tools: Arc::new(Mutex::new(vec![])),
        }
    }

    fn take_tools(&self) -> Vec<String> {
        self.last_tools.lock().expect("lock").clone()
    }
}

impl AgentClient for RecordingClient {
    fn agent_turn(&self, request: &ai::domain::AskRequest) -> Result<ClientResponse, AgentError> {
        *self.last_tools.lock().expect("lock") = request
            .tools
            .iter()
            .map(|t| t.as_str().to_string())
            .collect();
        Ok(ClientResponse::AgentTurnResult {
            id: "test-id".into(),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "ok".into(),
            },
            tool_calls: vec![],
        })
    }
}

#[test]
fn resolve_read_only_sends_safe_tools_to_aibe() {
    let client = RecordingClient::new();
    let presenter = StdoutPresenter::new(None);
    let ask = Ask::new(
        &client,
        &presenter,
        None::<&ai::adapters::outbound::FileLogTail>,
    );
    let resolved =
        resolve_tools(Some("@read-only"), &ConfigToolsTokens::default()).expect("resolve");
    ask.run(
        "use tool".to_string(),
        AskRunOptions {
            resolved_tools: resolved,
            verbose_tools: false,
            llm_profile: None,
            external_command_names: Vec::new(),
        },
    )
    .expect("ask");
    assert_eq!(
        client.take_tools(),
        vec![
            "read_file".to_string(),
            "list_dir".to_string(),
            "grep".to_string(),
            "git_diff".to_string(),
            "git_status".to_string()
        ]
    );
}

#[test]
fn cli_none_overrides_config_read_only() {
    let cfg = ConfigToolsTokens(vec!["@read-only".into()]);
    let resolved = resolve_tools(Some("none"), &cfg).expect("resolve");
    assert!(resolved.allowlist.is_empty());
}

#[test]
fn unknown_tool_errors_without_connect() {
    let err = resolve_tools(Some("nope"), &ConfigToolsTokens::default()).unwrap_err();
    assert!(matches!(err, ToolsResolveError::UnknownTool(_)));
}

#[test]
fn presenter_max_tool_rounds_and_verbose_tools_contract() {
    let huge = "z".repeat(MAX_TOOL_OUTPUT_BYTES + 200);
    let out = render_response(
        &ClientResponse::AgentTurnResult {
            id: "id".into(),
            status: AgentTurnStatus::MaxToolRounds,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "partial reply".into(),
            },
            tool_calls: vec![ExecutedToolCall::ok(
                "c1".into(),
                ToolName::read_file(),
                json!({"path": "/etc/passwd"}),
                huge,
            )],
        },
        true,
    );
    assert_eq!(out.stdout.as_deref(), Some("partial reply"));
    assert!(
        out.stderr.iter().any(|l| l.contains("max tool rounds")),
        "expected warning on stderr: {:?}",
        out.stderr
    );
    assert!(
        out.stderr
            .iter()
            .any(|l| l.starts_with("ai: tool read_file") && l.contains("[truncated]")),
        "expected verbose tool line on stderr: {:?}",
        out.stderr
    );
}

#[test]
fn ask_with_filter_completes() {
    let client = RecordingClient::new();
    let presenter = StdoutPresenter::new(Some("cat".into()));
    let ask = Ask::new(
        &client,
        &presenter,
        None::<&ai::adapters::outbound::FileLogTail>,
    );
    let resolved = resolve_tools(None, &ConfigToolsTokens::default()).expect("resolve");
    ask.run(
        "filtered".to_string(),
        AskRunOptions {
            resolved_tools: resolved,
            verbose_tools: false,
            llm_profile: None,
            external_command_names: Vec::new(),
        },
    )
    .expect("ask");
}

#[test]
fn external_commands_warning_line() {
    let presenter = StdoutPresenter::new(None);
    presenter.show_external_commands(&["codex".into(), "claude".into()]);
}

#[test]
fn plan_ask_launch_cli_none_with_config_read_only() {
    let ask_tools = ConfigToolsTokens(vec!["@read-only".into()]);
    let plan = plan_ask_launch(
        &ask_tools,
        Some("none"),
        std::path::PathBuf::from("/tmp/x.sock"),
        false,
    )
    .expect("plan");
    assert!(plan.resolved_tools.allowlist.is_empty());
}
