#![cfg(unix)]

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ai::adapters::outbound::{render_response, AibeUnixClient, StdoutPresenter};
use ai::application::{plan_ask_launch, Ask, AskRunOptions};
use ai::domain::{resolve_tools, AskInput, ConfigToolsTokens, ToolsResolveError};
use ai::ports::outbound::{AgentClient, AgentError};
use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::ports::outbound::ToolsConfig;
use aibe::protocol::{ClientResponse, ProtocolMessageOut};
use tempfile::tempdir;
use tokio::runtime::Runtime;

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
    fn agent_turn(&self, input: &AskInput) -> Result<ClientResponse, AgentError> {
        *self.last_tools.lock().expect("lock") = input.tools.clone();
        Ok(ClientResponse::AgentTurnResult {
            id: "test-id".into(),
            status: "ok".into(),
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "ok".into(),
            },
            tool_calls: vec![],
        })
    }
}

#[test]
fn ask_reaches_mock_aibe() {
    let dir = tempdir().expect("tempdir");
    let socket_path = dir.path().join("ai-test.sock");

    let socket_for_server = socket_path.clone();
    thread::spawn(move || {
        let rt = Runtime::new().expect("runtime");
        rt.block_on(async {
            server::run(
                socket_for_server,
                Arc::new(MockLlm::new()),
                ToolsConfig::default(),
            )
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
        },
    )
    .expect("ask");
}

#[test]
fn resolve_read_only_sends_read_file_to_aibe() {
    let client = RecordingClient::new();
    let presenter = StdoutPresenter;
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
        },
    )
    .expect("ask");
    assert_eq!(client.take_tools(), vec!["read_file".to_string()]);
}

#[test]
fn cli_none_overrides_config_read_only() {
    let cfg = ConfigToolsTokens(vec!["@read-only".into()]);
    let resolved = resolve_tools(Some("none"), &cfg).expect("resolve");
    assert!(resolved.names.is_empty());
}

#[test]
fn unknown_tool_errors_without_connect() {
    let err = resolve_tools(Some("nope"), &ConfigToolsTokens::default()).unwrap_err();
    assert!(matches!(err, ToolsResolveError::UnknownTool(_)));
}

#[test]
fn presenter_max_tool_rounds_and_verbose_tools_contract() {
    use aibe::protocol::ProtocolMessageOut;
    use serde_json::json;

    let huge = "z".repeat(aibe::ports::outbound::DEFAULT_MAX_TOOL_OUTPUT_BYTES + 200);
    let out = render_response(
        &ClientResponse::AgentTurnResult {
            id: "id".into(),
            status: "max_tool_rounds".into(),
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "partial reply".into(),
            },
            tool_calls: vec![json!({
                "name": "read_file",
                "status": "ok",
                "arguments": {"path": "/etc/passwd"},
                "output": huge
            })],
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
fn plan_ask_launch_cli_none_with_config_read_only() {
    let ask_tools = ConfigToolsTokens(vec!["@read-only".into()]);
    let plan = plan_ask_launch(
        &ask_tools,
        Some("none"),
        std::path::PathBuf::from("/tmp/x.sock"),
        false,
    )
    .expect("plan");
    assert!(plan.resolved_tools.names.is_empty());
}
