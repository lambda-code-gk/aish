use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ai::adapters::outbound::{HumanTaskFileStore, SystemHumanTaskTimeFormatter};
use ai::application::{HumanTaskCoordinator, HumanTaskParentInput, HumanTaskStatus};
use ai::domain::human_task_checkpoint::HumanTaskId;
use ai::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, HumanTaskIdentity,
};
use aibe::adapters::outbound::tools::{DefaultToolRegistry, HumanTaskTool};
use aibe::application::server;
use aibe::application::tool_round::{RoundOutcome, ToolRoundExecutor};
use aibe::domain::{
    ChatMessage, ClientCwd, ExecutedToolCall, LlmStepResult, ToolCall, ToolName, ToolResult,
};
use aibe::ports::outbound::{
    HumanTaskGate, LlmError, LlmProvider, MemoryConfig, NoopLlmCallTracer, ProfileRegistry,
    TerminationCapability, ToolDefinition, ToolExecutionContext, ToolExecutor, ToolsConfig,
};
use aibe_client::{
    agent_turn_on_stream_with_callbacks, AgentTurnCallbacks, ShellExecApprovalDecision,
    ToolApprovalDecision,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ExecutionMode, HandoffExecutionOutcome,
    HumanTaskRequest, HumanTaskResult, PostHandoffObservation, ProtocolMessage, RequestContext,
    ShellExecApprovalOrigin, ShellLogRange, ToolApprovalOrigin, HUMAN_TASK, READ_FILE,
};
use async_trait::async_trait;

struct Identity;
impl HumanTaskIdentity for Identity {
    fn new_task_id(&self) -> HumanTaskId {
        HumanTaskId::parse("ht-20260714-7f31c2").unwrap()
    }
    fn now_ms(&self) -> u64 {
        20
    }
}
struct SuspendedLauncher;
impl HumanShellLauncher for SuspendedLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        Err(HumanShellLaunchError::Suspended {
            returned: Box::new(HumanShellReturn {
                outcome: ai::ports::outbound::HumanShellOutcome::Suspended,
                suspend_reason: Some("approval needed".into()),
                exit_code: Some(0),
                final_cwd: request.cwd.clone(),
                shell_session_id: "shell-1".into(),
                shell_session_dir: PathBuf::new(),
                shell_log_start: 1,
                shell_log_end: 2,
            }),
            reason: Some("approval needed".into()),
        })
    }
}
struct Observer;
impl EnvironmentObserver for Observer {
    fn observe(
        &self,
        cwd: &Path,
        _: u64,
        _: Option<u64>,
        _: Option<&Path>,
    ) -> PostHandoffObservation {
        PostHandoffObservation {
            cwd_exists: true,
            cwd: cwd.display().to_string(),
            git_head: None,
            git_branch: None,
            git_status: Some("clean".into()),
            shell_log_tail: None,
            shell_log_truncated: None,
            observation_errors: vec![],
            human_task_evidence: None,
        }
    }
}

struct CoordinatorGate {
    history: PathBuf,
    cwd: PathBuf,
}
#[async_trait]
impl HumanTaskGate for CoordinatorGate {
    async fn execute_human_task(
        &self,
        _: &str,
        request: HumanTaskRequest,
    ) -> Option<HumanTaskResult> {
        let store = HumanTaskFileStore::new(self.history.clone());
        Some(
            HumanTaskCoordinator::new(&store, &Identity, &SuspendedLauncher, &Observer).execute(
                request,
                HumanTaskParentInput {
                    ai_session_id: "s1".into(),
                    conversation_id: "c1".into(),
                    turn_id: "t1".into(),
                    user_request: "help me".into(),
                    cwd: self.cwd.clone(),
                    llm_profile: "fast".into(),
                    runtime_dir: self.cwd.join("runtime"),
                },
                &AtomicBool::new(false),
            ),
        )
    }
}

struct ScriptedLlm {
    calls: AtomicUsize,
}
#[async_trait]
impl LlmProvider for ScriptedLlm {
    async fn complete(&self, _: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        panic!("next LLM call is forbidden after suspend")
    }
    async fn complete_with_tools(
        &self,
        _: &[ChatMessage],
        _: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        assert_eq!(
            self.calls.fetch_add(1, Ordering::SeqCst),
            0,
            "only one LLM call"
        );
        Ok(LlmStepResult::with_tool_calls(
            "",
            vec![
                ToolCall {
                    id: "human".into(),
                    name: HUMAN_TASK.into(),
                    arguments: serde_json::json!({"objective":"review"}),
                    provider_extras: None,
                },
                ToolCall {
                    id: "after".into(),
                    name: READ_FILE.into(),
                    arguments: serde_json::json!({"path":"never"}),
                    provider_extras: None,
                },
            ],
        ))
    }
}
struct CountingTool(Arc<AtomicUsize>);
#[async_trait]
impl ToolExecutor for CountingTool {
    fn name(&self) -> ToolName {
        ToolName::read_file()
    }
    async fn execute(
        &self,
        id: &str,
        args: &serde_json::Value,
        _: u64,
        _: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        self.0.fetch_add(1, Ordering::SeqCst);
        (
            ExecutedToolCall::ok(id.into(), self.name(), args.clone(), "unexpected".into()),
            ToolResult {
                tool_call_id: id.into(),
                content: "unexpected".into(),
                is_error: false,
            },
        )
    }
}

async fn run_slice(
    history: PathBuf,
    cwd: PathBuf,
) -> (RoundOutcome, Arc<ScriptedLlm>, Arc<AtomicUsize>) {
    let llm = Arc::new(ScriptedLlm {
        calls: AtomicUsize::new(0),
    });
    let later = Arc::new(AtomicUsize::new(0));
    let registry = Arc::new(
        DefaultToolRegistry::from_executors(vec![
            Arc::new(HumanTaskTool) as Arc<dyn ToolExecutor>,
            Arc::new(CountingTool(later.clone())),
        ])
        .unwrap(),
    );
    let executor = ToolRoundExecutor::new(
        llm.clone(),
        registry,
        ToolsConfig::default(),
        Arc::new(NoopLlmCallTracer),
    );
    let ctx = ToolExecutionContext::new(ClientCwd::parse(&cwd.display().to_string()).unwrap())
        .with_execution_mode(ExecutionMode::Collaborative)
        .with_human_task_gate(Arc::new(CoordinatorGate { history, cwd }));
    let outcome = executor
        .run_one_round(
            &[ChatMessage::user("help")],
            &[ToolName::human_task(), ToolName::read_file()],
            &[],
            &ctx,
            &[],
            None,
            None,
        )
        .await
        .unwrap();
    (outcome, llm, later)
}

#[tokio::test]
async fn human_task_suspend_checkpoint_vertical_e2e() {
    let dir = Arc::new(tempfile::tempdir().unwrap());
    let previous_home = std::env::var_os("HOME");
    std::env::set_var("HOME", dir.path());
    let socket_path = dir.path().join("0063.sock");
    let history = dir.path().join("history");
    let llm = Arc::new(ScriptedLlm {
        calls: AtomicUsize::new(0),
    });
    let profiles = ProfileRegistry::single(
        "default",
        llm.clone() as Arc<dyn LlmProvider>,
        TerminationCapability::summary_prompt_only(),
    );
    let server_dir = Arc::clone(&dir);
    let server_socket = socket_path.clone();
    let server_task = tokio::spawn(async move {
        server::run(
            server_socket,
            server_dir.path().join("config.toml"),
            profiles,
            ToolsConfig::default(),
            Vec::new(),
            "default".into(),
            server_dir.path().join("conversations"),
            MemoryConfig::default(),
        )
        .await
        .unwrap();
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !socket_path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let client_socket = socket_path.clone();
    let cwd = dir.path().to_path_buf();
    let callback_cwd = cwd.clone();
    let callback_history = history.clone();
    let response = tokio::task::spawn_blocking(move || {
        let stream = std::os::unix::net::UnixStream::connect(client_socket).unwrap();
        let request = ClientRequest::AgentTurn {
            id: "turn-0063".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "help me".into(),
            }],
            tools: vec![HUMAN_TASK.into(), READ_FILE.into()],
            client_tools: Vec::new(),
            context: RequestContext {
                cwd: Some(cwd.display().to_string()),
                execution_mode: ExecutionMode::Collaborative,
                ..Default::default()
            },
            llm_profile: None,
        };
        let callbacks = AgentTurnCallbacks::new(
            |_| ShellExecApprovalDecision {
                approved: false,
                approval_origin: ShellExecApprovalOrigin::UiNo,
                handoff_result: None,
                handoff_error: None,
            },
            |_| ToolApprovalDecision::Denied(ToolApprovalOrigin::UiNo),
        )
        .with_human_task(move |prompt: aibe_client::HumanTaskExecutionPrompt| {
            let store = HumanTaskFileStore::new(callback_history.clone());
            Some(
                HumanTaskCoordinator::new(&store, &Identity, &SuspendedLauncher, &Observer)
                    .execute(
                        prompt.request,
                        HumanTaskParentInput {
                            ai_session_id: "s1".into(),
                            conversation_id: "c1".into(),
                            turn_id: "turn-0063".into(),
                            user_request: "help me".into(),
                            cwd: callback_cwd.clone(),
                            llm_profile: "default".into(),
                            runtime_dir: callback_cwd.join("runtime"),
                        },
                        &AtomicBool::new(false),
                    ),
            )
        });
        agent_turn_on_stream_with_callbacks(stream, request, callbacks).unwrap()
    })
    .await
    .unwrap();
    let ClientResponse::AgentTurnResult {
        status,
        assistant_message,
        tool_calls,
        ..
    } = response
    else {
        panic!("expected agent turn result")
    };
    assert_eq!(status, AgentTurnStatus::Ok);
    assert!(assistant_message
        .content
        .starts_with("Human Task suspended."));
    assert!(assistant_message
        .content
        .contains("Cancel:\n  ai human-task cancel --yes"));
    assert!(assistant_message
        .content
        .contains("Resume:\n  ai human-task resume"));
    assert_eq!(
        tool_calls.len(),
        1,
        "same-round following tool must not run"
    );
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    let reopened = HumanTaskFileStore::new(history);
    let text = HumanTaskStatus::new(&reopened, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(text.contains("ht-20260714-7f31c2"));
    assert!(text.contains("approval needed"));
    server_task.abort();
    let _ = server_task.await;
    if let Some(home) = previous_home {
        std::env::set_var("HOME", home);
    } else {
        std::env::remove_var("HOME");
    }
}

#[tokio::test]
async fn human_task_suspend_stops_agent_turn_without_llm() {
    let dir = tempfile::tempdir().unwrap();
    let (outcome, llm, later) = run_slice(dir.path().join("history"), dir.path().into()).await;
    let RoundOutcome::SuspendTurn { executed, .. } = outcome else {
        panic!("expected suspended turn")
    };
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0].arguments, serde_json::json!({"redacted": true}));
    assert!(executed[0].output.is_none());
    assert_eq!(llm.calls.load(Ordering::SeqCst), 1);
    assert_eq!(later.load(Ordering::SeqCst), 0);
}

#[test]
fn human_task_suspend_is_explicit_tool_only() {
    let legacy = aibe_protocol::HumanHandoffResult {
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        requested_command: Some("echo ok".into()),
        requested_command_completion: aibe_protocol::RequestedCommandCompletion::Unknown,
        human_shell_exit_code: Some(0),
        final_shell_cwd: Some("/tmp".into()),
        shell_log_range: Some(ShellLogRange {
            start: 0,
            end: Some(1),
        }),
        observation: None,
    };
    let json = serde_json::to_string(&legacy).unwrap();
    assert!(!json.contains("suspended"));
    assert!(!json.contains("task_id"));
}
