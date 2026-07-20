//! Spec 0068 production composition vertical E2E.

#![cfg(unix)]

use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread;

use ai::adapters::outbound::{render_response, AibeUnixClient, ShellExecRenderOptions};
use ai::application::{Ask, AskRunOptions};
use ai::domain::{resolve_tools, ConfigToolsTokens, RequestContextInput};
use ai::ports::outbound::{AgentClient, AgentError, Presenter};
use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm, StaticCapabilityPolicy};
use aibe::application::{basic_pack_arc, build_default_tool_registry, RequestService};
use aibe::domain::{FeatureRegistry, LlmStepResult, ToolCall, READ_FILE, SHELL_EXEC};
use aibe::ports::inbound::{ClientRequestHandler, ShutdownCoordinator};
use aibe::ports::outbound::{
    ProfileRegistry, ShellExecApprovalMode, ShellExecConfig, TerminationCapability, ToolsConfig,
};
use aibe_protocol::ClientResponse;
use serde_json::json;

struct PairClient {
    stream: Mutex<Option<UnixStream>>,
    fallback: Arc<RequestService>,
}

impl AgentClient for PairClient {
    fn agent_turn(&self, request: &ai::domain::AskRequest) -> Result<ClientResponse, AgentError> {
        let stream = self
            .stream
            .lock()
            .map_err(|error| AgentError::Request(error.to_string()))?
            .take()
            .ok_or_else(|| AgentError::Request("stream already used".into()))?;
        let wire = AibeUnixClient::to_client_request(request);
        match aibe_client::agent_turn_on_stream(stream, wire.clone(), |_| {
            panic!("approval is disabled by production policy")
        }) {
            Ok(response) => Ok(response),
            Err(error) if error.to_string().contains("Operation not permitted") => {
                Ok(tokio::runtime::Runtime::new()
                    .map_err(|runtime_error| AgentError::Request(runtime_error.to_string()))?
                    .block_on(self.fallback.handle(wire, None)))
            }
            Err(error) => Err(AgentError::Request(error.to_string())),
        }
    }
}

struct NoopPresenter;

impl Presenter for NoopPresenter {
    fn show_tools_startup(&self, _: &ai::domain::ToolsStartupLine) {}
    fn show_external_commands(&self, _: &[String]) {}
    fn show_progress(&self, _: &str, _: Option<&str>) {}
    fn show_stream_chunk(&self, _: &str) {}
    fn show_response(&self, _: &ClientResponse, _: bool, _: bool) {}
    fn show_error(&self, _: &str) {}
}

fn contract() -> serde_json::Value {
    json!({
        "goal": "create and observe artifact.txt",
        "criteria": [{
            "id": "c1",
            "description": "artifact is observed after creation",
            "deliverable_is_plan": false
        }],
        "constraints": ["request local"],
        "deliverables": ["artifact.txt"],
        "verification": ["read artifact.txt after creation"]
    })
}

fn contract_only() -> String {
    json!({
        "aish_task_completion": {"contract": contract()},
        "deliverable": ""
    })
    .to_string()
}

fn evaluated(status: &str, evidence_ids: &[&str], next: Option<&str>, body: &str) -> String {
    json!({
        "aish_task_completion": {
            "contract": contract(),
            "evaluation": {
                "criteria": [{
                    "criterion_id": "c1",
                    "status": status,
                    "evidence_ids": evidence_ids,
                    "required_evidence": if status == "unsatisfied" { json!(["post-change read"]) } else { json!([]) }
                }],
                "next_objective": next,
                "needs_user": null,
                "blocked": null
            }
        },
        "deliverable": body
    })
    .to_string()
}

#[test]
fn task_completion_vertical_e2e() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "write".into(),
                name: SHELL_EXEC.into(),
                arguments: json!({"command": "touch", "args": ["artifact.txt"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only(evaluated(
            "unsatisfied",
            &[],
            Some("read artifact.txt now"),
            "created; verification pending",
        )),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "observe".into(),
                name: READ_FILE.into(),
                arguments: json!({"path": "artifact.txt"}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only(evaluated("satisfied", &["e2"], None, "artifact verified")),
    ]));
    let profiles = ProfileRegistry::single(
        "default",
        llm.clone(),
        TerminationCapability::summary_prompt_only(),
    );
    let tools = ToolsConfig {
        shell_exec: ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["touch".into()],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        },
        read_file: aibe::ports::outbound::ReadFileConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
        },
        ..Default::default()
    };
    let strategy = tools.termination_strategy;
    let (rpc_extension, turn_hook) = basic_pack_arc();
    let conversation_store = Arc::new(ConversationStore::new(dir.path().join("conversations")));
    let service = Arc::new(RequestService::new(
        profiles,
        build_default_tool_registry(&tools, &[]),
        tools,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".into(),
        conversation_store.clone(),
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    ));
    let (client_stream, server_stream) = UnixStream::pair().expect("mock socket pair");
    server_stream.set_nonblocking(true).expect("nonblocking");
    let handler: Arc<dyn ClientRequestHandler> = service.clone();
    let server = thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(async move {
                let stream = tokio::net::UnixStream::from_std(server_stream).expect("tokio stream");
                aibe::adapters::inbound::unix_socket_server::serve_connected_stream(
                    stream,
                    handler,
                    ShutdownCoordinator::new(),
                )
                .await
                .expect("production connection");
            });
    });
    let client = PairClient {
        stream: Mutex::new(Some(client_stream)),
        fallback: service,
    };
    let presenter = NoopPresenter;
    let ask = Ask::new(
        &client,
        &presenter,
        None::<&ai::adapters::outbound::FileLogTail>,
    );
    let outcome = ask
        .run(
            "create artifact".into(),
            AskRunOptions {
                resolved_tools: resolve_tools(
                    Some("shell_exec,read_file"),
                    &ConfigToolsTokens::default(),
                )
                .expect("tools"),
                verbose_tools: false,
                llm_profile: None,
                external_command_names: vec![],
                shell_log_tail_bytes: 0,
                client_cwd: Some(dir.path().to_path_buf()),
                ai_session_id: Some("session-0068".into()),
                conversation_id: Some("conversation-0068".into()),
                client_tools: vec![],
                replay_events: vec![],
                replay_manifest_block: None,
                request_context: RequestContextInput::default(),
            },
        )
        .expect("one ai Ask command");

    assert!(dir.path().join("artifact.txt").exists());
    let output = render_response(&outcome.response, false, ShellExecRenderOptions::default())
        .stdout
        .expect("report");
    assert!(output.contains("Task completion: Done"));
    assert!(output.contains("Queries used: 2"));
    assert!(output.contains("Evidence e2 source=Observation verified=true"));
    let calls = llm.recorded_calls();
    assert_eq!(calls.len(), 4);
    let second_query = calls[2]
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(second_query.contains("Next objective: read artifact.txt now"));
    assert!(second_query.contains("post-change read"));
    assert!(!second_query.contains("create artifact\n"));
    let snapshot = conversation_store
        .load_snapshot("session-0068", "conversation-0068")
        .expect("load conversation")
        .expect("saved conversation");
    assert!(snapshot
        .messages
        .iter()
        .all(|message| !message.content.contains("Task completion control:")));
    drop(client);
    server.join().expect("server thread");
}
