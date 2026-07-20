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
use aibe::domain::{FeatureRegistry, LlmStepResult, ToolCall, READ_FILE, SHELL_EXEC, WRITE_FILE};
use aibe::ports::inbound::{ClientRequestHandler, ShutdownCoordinator};
use aibe::ports::outbound::{
    FileWriteApprovalMode, HumanTaskGate, ProfileRegistry, TerminationCapability, ToolsConfig,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ExecutionMode, HandoffExecutionOutcome,
    HumanTaskRequest, HumanTaskResult, PostHandoffObservation, ProtocolMessage, RequestContext,
    ShellLogRange, HUMAN_TASK,
};
use async_trait::async_trait;
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
        "task_kind": "execution",
        "criteria": [{
            "id": "c1",
            "description": "artifact is observed after creation",
            "deliverable_is_plan": false,
            "observes_targets": ["artifact.txt"]
        }],
        "constraints": ["request local"],
        "deliverables": ["artifact.txt"],
        "verification": ["read artifact.txt after creation"],
        "verification_tools": ["read_file"]
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

fn request_service(llm: Arc<ScriptedMockLlm>, root: &std::path::Path) -> RequestService {
    let profiles =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let tools = ToolsConfig::default();
    let strategy = tools.termination_strategy;
    let (rpc_extension, turn_hook) = basic_pack_arc();
    RequestService::new(
        profiles,
        build_default_tool_registry(&tools, &[]),
        tools,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".into(),
        Arc::new(ConversationStore::new(root.join("conversations"))),
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    )
}

struct SuspendedHumanTaskGate;

#[async_trait]
impl HumanTaskGate for SuspendedHumanTaskGate {
    async fn execute_human_task(&self, _: &str, task: HumanTaskRequest) -> Option<HumanTaskResult> {
        Some(HumanTaskResult {
            status: HandoffExecutionOutcome::Suspended,
            task,
            verified: false,
            human_shell_exit_code: Some(0),
            final_shell_cwd: Some("/tmp".into()),
            shell_log_range: Some(ShellLogRange {
                start: 1,
                end: Some(2),
            }),
            observation: Some(PostHandoffObservation {
                cwd_exists: true,
                cwd: "/tmp".into(),
                git_head: None,
                git_branch: None,
                git_status: None,
                shell_log_tail: None,
                shell_log_truncated: None,
                observation_errors: vec![],
                human_task_evidence: None,
            }),
            error: None,
            task_id: Some("ht-20260720-abcdef".into()),
            suspend_reason: Some("human review required".into()),
        })
    }
}

#[test]
fn task_completion_vertical_e2e() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "write".into(),
                name: WRITE_FILE.into(),
                arguments: json!({
                    "path": "artifact.txt",
                    "mode": "create",
                    "content": "created\n"
                }),
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
        file_write: aibe::ports::outbound::FileWriteConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
            approval: FileWriteApprovalMode::Always,
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
                    Some("write_file,read_file"),
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
                request_context: RequestContextInput {
                    task_completion: true,
                    ..Default::default()
                },
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
    assert!(second_query.contains("Fixed contract:"));
    assert!(second_query.contains("Existing evidence:"));
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

#[tokio::test]
async fn effect_tools_without_explicit_signal_keep_simple_question_inactive() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
        "plain answer",
    )]));
    let service = request_service(llm, dir.path());
    let response = service
        .handle(
            ClientRequest::AgentTurn {
                id: "inactive-0068".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "what is this project?".into(),
                }],
                tools: vec![SHELL_EXEC.into()],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        completion_report,
        ..
    } = response
    else {
        panic!("simple question must remain a normal agent turn")
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(assistant_message.content, "plain answer");
    assert!(completion_report.is_none());
}

#[tokio::test]
async fn inactive_turn_passes_plain_text_mentioning_task_completion_marker() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let explanation = "Task Completion uses the aish_task_completion JSON envelope when enabled.";
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
        explanation,
    )]));
    let service = request_service(llm, dir.path());
    let response = service
        .handle(
            ClientRequest::AgentTurn {
                id: "inactive-marker-0068".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "explain Task Completion to me".into(),
                }],
                tools: vec![SHELL_EXEC.into()],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        completion_report,
        ..
    } = response
    else {
        panic!("inactive explanation must remain a normal agent turn")
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert_eq!(assistant_message.content, explanation);
    assert!(completion_report.is_none());
}

#[tokio::test]
async fn second_query_human_task_suspend_is_preserved() {
    let dir = tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("tempdir");
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::text_only(evaluated(
            "unsatisfied",
            &[],
            Some("ask a human to finish verification"),
            "verification pending",
        )),
        LlmStepResult::with_tool_calls(
            contract_only(),
            vec![ToolCall {
                id: "human".into(),
                name: HUMAN_TASK.into(),
                arguments: json!({"objective": "verify artifact manually"}),
                provider_extras: None,
            }],
        ),
    ]));
    let service = request_service(llm.clone(), dir.path());
    let response = service
        .handle_with_events(
            ClientRequest::AgentTurn {
                id: "suspend-second-0068".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "create and verify artifact".into(),
                }],
                tools: vec![SHELL_EXEC.into(), HUMAN_TASK.into()],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some(dir.path().display().to_string()),
                    execution_mode: ExecutionMode::Collaborative,
                    task_completion: true,
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
            None,
            None,
            Some(Arc::new(SuspendedHumanTaskGate)),
            None,
            None,
        )
        .await;
    let ClientResponse::AgentTurnResult { status, .. } = response else {
        panic!("typed suspension must not become InvalidRequest")
    };
    assert_eq!(status, AgentTurnStatus::Suspended);
    assert_eq!(llm.recorded_calls().len(), 2);
}
