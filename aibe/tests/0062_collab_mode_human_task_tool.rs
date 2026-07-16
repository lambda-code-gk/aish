//! 0062 Collaborative Mode `human_task` の socket 縦断・fail-closed テスト。

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ai::application::ExecuteHumanTask;
use ai::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn,
};
use aibe::adapters::inbound::connection_human_task::ConnectionHumanTaskGate;
use aibe::adapters::outbound::MockLlm;
use aibe::application::server;
use aibe::domain::{ChatMessage, LlmStepResult, MessageRole, ToolCall};
use aibe::ports::outbound::{
    HumanTaskGate, LlmError, LlmProvider, MemoryConfig, ProfileRegistry, TerminationCapability,
    ToolDefinition, ToolsConfig,
};
use aibe_client::{
    agent_turn_on_stream_with_callbacks, AgentTurnCallbacks, ShellExecApprovalDecision,
    ToolApprovalDecision,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ErrorCode, ExecutionMode,
    HandoffExecutionOutcome, HumanTaskEvidence, HumanTaskRequest, HumanTaskResult,
    PostHandoffObservation, ProtocolMessage, RequestContext, ShellExecApprovalOrigin,
    ShellLogRange, ToolApprovalOrigin, HUMAN_TASK, SHELL_EXEC,
};
use async_trait::async_trait;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Default)]
struct RecordingLauncher {
    requests: Mutex<Vec<HumanShellLaunchRequest>>,
}

impl HumanShellLauncher for RecordingLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        self.requests
            .lock()
            .expect("launcher requests")
            .push(request.clone());
        Ok(HumanShellReturn {
            outcome: ai::ports::outbound::HumanShellOutcome::Done,
            exit_code: Some(0),
            final_cwd: request.cwd.clone(),
            shell_session_id: "fake-human-shell".into(),
            shell_session_dir: PathBuf::new(),
            shell_log_start: 10,
            shell_log_end: 12,
        })
    }
}

struct RecordingObserver;

impl EnvironmentObserver for RecordingObserver {
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
            observation_errors: Vec::new(),
            human_task_evidence: Some(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: false,
            }),
        }
    }
}

struct RecordingHumanTaskLlm {
    calls: Mutex<Vec<(Vec<ChatMessage>, Vec<String>)>>,
}

impl RecordingHumanTaskLlm {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LlmProvider for RecordingHumanTaskLlm {
    async fn complete(&self, _: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        Ok(ChatMessage::assistant("parent continued"))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let mut calls = self.calls.lock().expect("llm calls");
        calls.push((
            messages.to_vec(),
            tools.iter().map(|tool| tool.name.clone()).collect(),
        ));
        if calls.len() == 1 {
            return Ok(LlmStepResult::with_tool_calls(
                "",
                vec![ToolCall {
                    id: "human-call-1".into(),
                    name: HUMAN_TASK.into(),
                    arguments: json!({
                        "objective": "inspect workspace",
                        "reason": "needs human judgment",
                        "instructions": ["review changes"],
                        "completion_criteria": ["review complete"]
                    }),
                    provider_extras: None,
                }],
            ));
        }
        Ok(LlmStepResult::text_only("parent continued"))
    }
}

#[tokio::test]
async fn collab_human_task_vertical_with_fakes() {
    let dir = Arc::new(tempfile::tempdir().expect("tempdir"));
    let socket_path = dir.path().join("0062.sock");
    let llm = Arc::new(RecordingHumanTaskLlm::new());
    let profiles = ProfileRegistry::single(
        "default",
        Arc::clone(&llm) as Arc<dyn LlmProvider>,
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
        .expect("server");
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !socket_path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("socket ready");

    let launcher = Arc::new(RecordingLauncher::default());
    let launcher_for_client = Arc::clone(&launcher);
    let client_socket = socket_path.clone();
    let cwd = dir.path().to_path_buf();
    let response = tokio::task::spawn_blocking(move || {
        let stream = std::os::unix::net::UnixStream::connect(client_socket).expect("connect");
        let request = ClientRequest::AgentTurn {
            id: "turn-0062".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "collaborate with me".into(),
            }],
            tools: vec![HUMAN_TASK.into()],
            client_tools: Vec::new(),
            context: RequestContext {
                cwd: Some(cwd.display().to_string()),
                execution_mode: ExecutionMode::Collaborative,
                ..Default::default()
            },
            llm_profile: None,
        };
        let callback_cwd = cwd.clone();
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
            Some(
                ExecuteHumanTask::new(&*launcher_for_client, &RecordingObserver).execute(
                    prompt.request,
                    callback_cwd.clone(),
                    callback_cwd.join("return-marker"),
                    &AtomicBool::new(false),
                ),
            )
        });
        agent_turn_on_stream_with_callbacks(stream, request, callbacks).expect("agent turn")
    })
    .await
    .expect("client task");

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
    assert_eq!(assistant_message.content, "parent continued");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, HUMAN_TASK);
    assert_eq!(tool_calls[0].status, aibe_protocol::ExecutedToolStatus::Ok);
    assert!(!tool_calls.iter().any(|call| call.name == SHELL_EXEC));

    let structured: aibe_protocol::HumanTaskResult = serde_json::from_str(
        tool_calls[0]
            .output
            .as_deref()
            .expect("structured tool output"),
    )
    .expect("human task result json");
    assert_eq!(structured.status, HandoffExecutionOutcome::Done);
    assert_eq!(structured.task.objective, "inspect workspace");
    assert_eq!(
        launcher.requests.lock().expect("launcher requests").len(),
        1
    );

    {
        let calls = llm.calls.lock().expect("llm calls");
        assert_eq!(
            calls.len(),
            2,
            "parent agent must continue to the next round"
        );
        assert_eq!(calls[0].1, [HUMAN_TASK]);
        let tool_result = calls[1]
            .0
            .iter()
            .find(|message| message.role == MessageRole::Tool)
            .expect("structured result returned to LLM");
        let round_result: aibe_protocol::HumanTaskResult =
            serde_json::from_str(&tool_result.content).expect("round result json");
        assert_eq!(round_result.status, HandoffExecutionOutcome::Done);
        assert!(calls[1].1.iter().all(|name| name != SHELL_EXEC));
    }

    server_task.abort();
    let _ = server_task.await;
}

fn gate_task() -> HumanTaskRequest {
    HumanTaskRequest {
        objective: "inspect workspace".into(),
        reason: None,
        instructions: Vec::new(),
        completion_criteria: Vec::new(),
    }
}

fn gate_result() -> HumanTaskResult {
    HumanTaskResult {
        status: HandoffExecutionOutcome::Done,
        task: gate_task(),
        verified: false,
        human_shell_exit_code: Some(0),
        final_shell_cwd: Some("/tmp".into()),
        shell_log_range: Some(ShellLogRange {
            start: 0,
            end: Some(1),
        }),
        observation: Some(PostHandoffObservation {
            cwd_exists: true,
            cwd: "/tmp".into(),
            git_head: None,
            git_branch: None,
            git_status: None,
            shell_log_tail: None,
            shell_log_truncated: None,
            observation_errors: Vec::new(),
            human_task_evidence: Some(HumanTaskEvidence {
                commands: Vec::new(),
                truncated: false,
            }),
        }),
        error: None,
        task_id: None,
        suspend_reason: None,
    }
}

enum CorruptResponse {
    PromptId,
    TurnId,
    ToolCallId,
    Decode,
}

async fn rejected_correlated_response(corruption: CorruptResponse) {
    let (gate_stream, client_stream) = tokio::net::UnixStream::pair().expect("socket pair");
    let (gate_reader, gate_writer) = gate_stream.into_split();
    let gate = Arc::new(ConnectionHumanTaskGate::new(
        "turn-0062-errors".into(),
        Arc::new(AsyncMutex::new(gate_writer)),
        Arc::new(AsyncMutex::new(BufReader::new(gate_reader).lines())),
        None,
        None,
    ));
    let pending = {
        let gate = Arc::clone(&gate);
        tokio::spawn(async move {
            gate.execute_human_task("human-call-errors", gate_task())
                .await
        })
    };
    let (client_reader, mut client_writer) = client_stream.into_split();
    let mut client_lines = BufReader::new(client_reader).lines();
    let prompt: ClientResponse = serde_json::from_str(
        &client_lines
            .next_line()
            .await
            .expect("read prompt")
            .expect("prompt line"),
    )
    .expect("prompt json");
    let ClientResponse::HumanTaskExecutionRequest {
        mut id,
        mut turn_id,
        mut tool_call_id,
        ..
    } = prompt
    else {
        panic!("human task prompt expected")
    };
    let line = match corruption {
        CorruptResponse::Decode => "{not-json}\n".to_string(),
        corruption => {
            match corruption {
                CorruptResponse::PromptId => id.push_str("-mismatch"),
                CorruptResponse::TurnId => turn_id.push_str("-mismatch"),
                CorruptResponse::ToolCallId => tool_call_id.push_str("-mismatch"),
                CorruptResponse::Decode => unreachable!(),
            }
            format!(
                "{}\n",
                serde_json::to_string(&ClientRequest::HumanTaskExecutionResult {
                    id,
                    turn_id,
                    tool_call_id,
                    result: gate_result(),
                })
                .expect("response json")
            )
        }
    };
    client_writer
        .write_all(line.as_bytes())
        .await
        .expect("write response");
    client_writer.flush().await.expect("flush response");
    assert_eq!(pending.await.expect("gate task"), None);
}

async fn duplicate_response_is_not_reused_by_next_waiter() {
    let (gate_stream, client_stream) = tokio::net::UnixStream::pair().expect("socket pair");
    let (gate_reader, gate_writer) = gate_stream.into_split();
    let gate = Arc::new(ConnectionHumanTaskGate::new(
        "turn-0062-duplicate".into(),
        Arc::new(AsyncMutex::new(gate_writer)),
        Arc::new(AsyncMutex::new(BufReader::new(gate_reader).lines())),
        None,
        None,
    ));
    let (client_reader, mut client_writer) = client_stream.into_split();
    let mut client_lines = BufReader::new(client_reader).lines();

    let first = {
        let gate = Arc::clone(&gate);
        tokio::spawn(async move { gate.execute_human_task("call-1", gate_task()).await })
    };
    let prompt_line = client_lines
        .next_line()
        .await
        .expect("read first prompt")
        .expect("first prompt");
    let ClientResponse::HumanTaskExecutionRequest {
        id,
        turn_id,
        tool_call_id,
        ..
    } = serde_json::from_str(&prompt_line).expect("first prompt json")
    else {
        panic!("human task prompt expected")
    };
    let response = ClientRequest::HumanTaskExecutionResult {
        id,
        turn_id,
        tool_call_id,
        result: gate_result(),
    };
    let duplicate_line = format!(
        "{}\n",
        serde_json::to_string(&response).expect("result json")
    );
    client_writer
        .write_all(duplicate_line.as_bytes())
        .await
        .expect("write first response");
    client_writer.flush().await.expect("flush first response");
    assert_eq!(first.await.expect("first gate"), Some(gate_result()));

    client_writer
        .write_all(duplicate_line.as_bytes())
        .await
        .expect("write duplicate");
    client_writer.flush().await.expect("flush duplicate");
    let second = {
        let gate = Arc::clone(&gate);
        tokio::spawn(async move { gate.execute_human_task("call-2", gate_task()).await })
    };
    let _second_prompt = client_lines
        .next_line()
        .await
        .expect("read second prompt")
        .expect("second prompt");
    assert_eq!(second.await.expect("second gate"), None);
}

#[tokio::test]
async fn human_task_errors_are_structured_and_fail_closed() {
    rejected_correlated_response(CorruptResponse::PromptId).await;
    rejected_correlated_response(CorruptResponse::TurnId).await;
    rejected_correlated_response(CorruptResponse::ToolCallId).await;
    rejected_correlated_response(CorruptResponse::Decode).await;
    duplicate_response_is_not_reused_by_next_waiter().await;

    let dir = Arc::new(tempfile::tempdir().expect("tempdir"));
    let socket_path = dir.path().join("0062-unsolicited.sock");
    let profiles = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
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
        .expect("server");
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !socket_path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("socket ready");

    let stream = tokio::net::UnixStream::connect(&socket_path)
        .await
        .expect("connect");
    let (reader, mut writer) = stream.into_split();
    let unsolicited = ClientRequest::HumanTaskExecutionResult {
        id: "no-prompt".into(),
        turn_id: "no-turn".into(),
        tool_call_id: "no-call".into(),
        result: gate_result(),
    };
    writer
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(&unsolicited).expect("unsolicited json")
            )
            .as_bytes(),
        )
        .await
        .expect("write unsolicited response");
    writer.flush().await.expect("flush unsolicited response");
    let mut lines = BufReader::new(reader).lines();
    let response: ClientResponse = serde_json::from_str(
        &lines
            .next_line()
            .await
            .expect("read error")
            .expect("error line"),
    )
    .expect("error json");
    let ClientResponse::Error { code, message, .. } = response else {
        panic!("stable protocol error expected")
    };
    assert_eq!(code, ErrorCode::InvalidRequest);
    assert_eq!(
        message,
        "human_task_execution_result must be sent during an active agent_turn"
    );

    server_task.abort();
    let _ = server_task.await;
}
