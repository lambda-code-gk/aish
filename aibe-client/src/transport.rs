//! NDJSON request/response transport（同一接続上の承認往復を含む）。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use aibe_protocol::{
    ClientRequest, ClientResponse, ClientToolResult, ClientToolResultStatus, ProgressPhase,
    ShellExecApprovalOrigin, ToolApprovalOrigin, ToolRiskClass,
};

use crate::unix_connect::connect_unix_stream;

/// `agent_turn` の connect 上限。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// `shell_exec` 承認 prompt の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecApprovalPrompt {
    pub prompt_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecApprovalDecision {
    pub approved: bool,
    pub approval_origin: ShellExecApprovalOrigin,
    pub handoff_result: Option<aibe_protocol::HumanHandoffResult>,
    pub handoff_error: Option<aibe_protocol::HumanHandoffFailure>,
}

fn validate_handoff_error_fields(error: &aibe_protocol::HumanHandoffFailure) -> Result<(), String> {
    if error.code.trim().is_empty() {
        return Err("handoff_error.code must not be empty".into());
    }
    if error.code != "human_handoff_failed" {
        return Err(format!(
            "handoff_error.code must be human_handoff_failed, got {}",
            error.code
        ));
    }
    if error.message.trim().is_empty() {
        return Err("handoff_error.message must not be empty".into());
    }
    Ok(())
}

/// `ShellExecApprovalDecision` の protocol invariant を検証する。
pub fn validate_shell_exec_approval_decision(
    decision: &ShellExecApprovalDecision,
) -> Result<(), String> {
    let has_result = decision.handoff_result.is_some();
    let has_error = decision.handoff_error.is_some();
    let collaborative = decision.approval_origin == ShellExecApprovalOrigin::CollaborativeHandoff;

    if has_result && has_error {
        return Err("handoff_result and handoff_error cannot both be set".into());
    }
    if !collaborative && (has_result || has_error) {
        return Err("handoff fields require CollaborativeHandoff origin".into());
    }

    if collaborative {
        return match (decision.approved, has_result, has_error) {
            (true, true, false) => Ok(()),
            (false, false, true) => decision
                .handoff_error
                .as_ref()
                .map(validate_handoff_error_fields)
                .unwrap_or(Ok(())),
            (true, false, false) => {
                Err("collaborative handoff success requires handoff_result".into())
            }
            (false, false, false) => {
                Err("collaborative handoff failure requires handoff_error".into())
            }
            (false, true, _) => Err("handoff success requires approved=true".into()),
            (true, _, true) => Err("handoff failure requires approved=false".into()),
        };
    }

    Ok(())
}

/// write-like tool 承認 prompt の内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalPrompt {
    pub prompt_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub risk_class: ToolRiskClass,
    pub summary: String,
    pub paths: Vec<String>,
    pub preview: String,
    pub preview_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskExecutionPrompt {
    pub prompt_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub request: aibe_protocol::HumanTaskRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalDecision {
    Approved(ToolApprovalOrigin),
    Denied(ToolApprovalOrigin),
    Unavailable,
}

impl ToolApprovalDecision {
    pub fn to_wire(self) -> (bool, ToolApprovalOrigin) {
        match self {
            Self::Approved(origin) => (true, origin),
            Self::Denied(origin) => (false, origin),
            Self::Unavailable => (false, ToolApprovalOrigin::Unavailable),
        }
    }
}

/// `agent_turn` 中の承認 callback 集約（設計 §14.4）。
pub struct AgentTurnCallbacks<
    S,
    T,
    H = fn(HumanTaskExecutionPrompt) -> Option<aibe_protocol::HumanTaskResult>,
> {
    pub on_shell_exec: S,
    pub on_tool: T,
    pub on_human_task: H,
}

impl<S, T> AgentTurnCallbacks<S, T> {
    pub fn new(on_shell_exec: S, on_tool: T) -> Self {
        Self {
            on_shell_exec,
            on_tool,
            on_human_task: unavailable_human_task,
        }
    }
}

impl<S, T, H> AgentTurnCallbacks<S, T, H> {
    pub fn with_human_task<N>(self, on_human_task: N) -> AgentTurnCallbacks<S, T, N> {
        AgentTurnCallbacks {
            on_shell_exec: self.on_shell_exec,
            on_tool: self.on_tool,
            on_human_task,
        }
    }
}

fn unavailable_human_task(_: HumanTaskExecutionPrompt) -> Option<aibe_protocol::HumanTaskResult> {
    None
}

pub fn shell_exec_only_callbacks<S>(
    on_shell_exec: S,
) -> AgentTurnCallbacks<S, fn(ToolApprovalPrompt) -> ToolApprovalDecision>
where
    S: FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
{
    AgentTurnCallbacks::new(on_shell_exec, deny_tool_approval)
}

fn deny_tool_approval(_: ToolApprovalPrompt) -> ToolApprovalDecision {
    ToolApprovalDecision::Denied(ToolApprovalOrigin::UiNo)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientToolCallRequest {
    pub id: String,
    pub turn_id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connect to aibe: {0}")]
    Connect(#[from] std::io::Error),
    #[error("serialize request: {0}")]
    Serialize(String),
    #[error("deserialize response: {0}")]
    Deserialize(String),
    #[error("unexpected response: {0}")]
    Unexpected(String),
}

pub fn send_request(stream: &mut UnixStream, request: &ClientRequest) -> std::io::Result<()> {
    let payload = serde_json::to_string(request).map_err(std::io::Error::other)?;
    writeln!(stream, "{payload}")?;
    stream.flush()?;
    Ok(())
}

pub fn send_cancel_request(
    stream: &mut UnixStream,
    id: impl Into<String>,
    turn_id: impl Into<String>,
) -> std::io::Result<()> {
    let request = ClientRequest::CancelTurn {
        id: id.into(),
        turn_id: turn_id.into(),
    };
    send_request(stream, &request)
}

pub fn send_route_turn_request(
    stream: &mut UnixStream,
    request: &ClientRequest,
) -> std::io::Result<()> {
    send_request(stream, request)
}

pub fn read_response_line(stream: &mut UnixStream) -> Result<ClientResponse, ClientError> {
    let mut reader = BufReader::new(stream);
    read_response_from_reader(&mut reader)
}

pub fn route_turn_on_stream(
    stream: UnixStream,
    request: ClientRequest,
) -> Result<ClientResponse, ClientError> {
    let mut writer = stream;
    let mut reader = BufReader::new(writer.try_clone().map_err(ClientError::Connect)?);
    send_route_turn_request(&mut writer, &request).map_err(ClientError::Connect)?;
    read_response_from_reader(&mut reader)
}

pub fn route_turn(
    socket_path: &Path,
    request: ClientRequest,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    route_turn_on_stream(stream, request)
}

/// `memory_apply` / `memory_query` など単発応答 RPC。
pub fn memory_request(
    socket_path: &Path,
    request: ClientRequest,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    memory_request_on_stream(stream, request)
}

pub fn memory_request_on_stream(
    stream: UnixStream,
    request: ClientRequest,
) -> Result<ClientResponse, ClientError> {
    let mut writer = stream;
    let mut reader = BufReader::new(writer.try_clone().map_err(ClientError::Connect)?);
    send_request(&mut writer, &request).map_err(ClientError::Connect)?;
    read_response_from_reader(&mut reader)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnProgressEvent {
    pub phase: ProgressPhase,
    pub message: Option<String>,
}

fn read_response_from_reader<R: BufRead>(reader: &mut R) -> Result<ClientResponse, ClientError> {
    let mut line = String::new();
    reader.read_line(&mut line).map_err(ClientError::Connect)?;
    if line.trim().is_empty() {
        return Err(ClientError::Unexpected("empty response line".into()));
    }
    serde_json::from_str(line.trim()).map_err(|e| ClientError::Deserialize(e.to_string()))
}

/// 接続済み stream 上で `agent_turn` と承認往復を行う（テスト・カスタム接続向け）。
pub fn agent_turn_with_events_on_stream(
    stream: UnixStream,
    request: ClientRequest,
    on_progress: impl FnMut(AgentTurnProgressEvent),
    on_stream: impl FnMut(String),
    on_approval: impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
) -> Result<ClientResponse, ClientError> {
    agent_turn_with_client_tools_on_stream(
        stream,
        request,
        on_progress,
        on_stream,
        |_| None,
        shell_exec_only_callbacks(on_approval),
    )
}

pub fn agent_turn_with_client_tools_on_stream(
    stream: UnixStream,
    request: ClientRequest,
    mut on_progress: impl FnMut(AgentTurnProgressEvent),
    mut on_stream: impl FnMut(String),
    mut on_client_tool: impl FnMut(ClientToolCallRequest) -> Option<ClientToolResult>,
    callbacks: AgentTurnCallbacks<
        impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
        impl FnMut(ToolApprovalPrompt) -> ToolApprovalDecision,
        impl FnMut(HumanTaskExecutionPrompt) -> Option<aibe_protocol::HumanTaskResult>,
    >,
) -> Result<ClientResponse, ClientError> {
    let mut on_shell_exec = callbacks.on_shell_exec;
    let mut on_tool = callbacks.on_tool;
    let mut on_human_task = callbacks.on_human_task;
    let mut writer = stream;
    let mut reader = BufReader::new(writer.try_clone().map_err(ClientError::Connect)?);
    send_request(&mut writer, &request).map_err(ClientError::Connect)?;

    loop {
        match read_response_from_reader(&mut reader)? {
            ClientResponse::Progress { phase, message, .. } => {
                on_progress(AgentTurnProgressEvent { phase, message });
            }
            ClientResponse::AssistantStreaming { delta, .. } => {
                on_stream(delta);
            }
            ClientResponse::ShellExecApprovalPrompt {
                id,
                turn_id,
                tool_call_id,
                command,
                args,
            } => {
                let decision = on_shell_exec(ShellExecApprovalPrompt {
                    prompt_id: id.clone(),
                    turn_id: turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    command: command.clone(),
                    args: args.clone(),
                });
                if let Err(reason) = validate_shell_exec_approval_decision(&decision) {
                    return Err(ClientError::Unexpected(format!(
                        "invalid shell_exec approval decision: {reason}"
                    )));
                }
                send_request(
                    &mut writer,
                    &ClientRequest::ShellExecApproval {
                        id,
                        turn_id,
                        tool_call_id,
                        approved: decision.approved,
                        approval_origin: decision.approval_origin,
                        handoff_result: decision.handoff_result,
                        handoff_error: decision.handoff_error,
                    },
                )
                .map_err(ClientError::Connect)?;
            }
            ClientResponse::ToolApprovalPrompt {
                id,
                turn_id,
                tool_call_id,
                tool_name,
                risk_class,
                summary,
                paths,
                preview,
                preview_truncated,
            } => {
                let decision = on_tool(ToolApprovalPrompt {
                    prompt_id: id.clone(),
                    turn_id: turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    risk_class,
                    summary: summary.clone(),
                    paths: paths.clone(),
                    preview: preview.clone(),
                    preview_truncated,
                });
                let (approved, approval_origin) = decision.to_wire();
                send_request(
                    &mut writer,
                    &ClientRequest::ToolApproval {
                        id,
                        turn_id,
                        tool_call_id,
                        approved,
                        approval_origin,
                    },
                )
                .map_err(ClientError::Connect)?;
            }
            ClientResponse::ClientToolCallRequested {
                id,
                turn_id,
                call_id,
                name,
                arguments,
            } => {
                let Some(result) = on_client_tool(ClientToolCallRequest {
                    id: id.clone(),
                    turn_id: turn_id.clone(),
                    call_id: call_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }) else {
                    let result = ClientToolResult {
                        id,
                        turn_id,
                        call_id,
                        status: ClientToolResultStatus::Error,
                        error_kind: None,
                        content: "client tool unavailable".into(),
                    };
                    send_request(&mut writer, &ClientRequest::ClientToolResult(result))
                        .map_err(ClientError::Connect)?;
                    continue;
                };
                send_request(&mut writer, &ClientRequest::ClientToolResult(result))
                    .map_err(ClientError::Connect)?;
            }
            ClientResponse::HumanTaskExecutionRequest {
                id,
                turn_id,
                tool_call_id,
                request,
            } => {
                let Some(result) = on_human_task(HumanTaskExecutionPrompt {
                    prompt_id: id.clone(),
                    turn_id: turn_id.clone(),
                    tool_call_id: tool_call_id.clone(),
                    request,
                }) else {
                    return Err(ClientError::Unexpected(
                        "human_task callback unavailable".into(),
                    ));
                };
                result.validate().map_err(|reason| {
                    ClientError::Unexpected(format!("invalid human_task result: {reason}"))
                })?;
                send_request(
                    &mut writer,
                    &ClientRequest::HumanTaskExecutionResult {
                        id,
                        turn_id,
                        tool_call_id,
                        result,
                    },
                )
                .map_err(ClientError::Connect)?;
            }
            cancelled @ ClientResponse::Cancelled { .. } => return Ok(cancelled),
            final_resp => return Ok(final_resp),
        }
    }
}

pub fn agent_turn_with_events(
    socket_path: &Path,
    request: ClientRequest,
    on_progress: impl FnMut(AgentTurnProgressEvent),
    on_stream: impl FnMut(String),
    on_approval: impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    agent_turn_with_events_on_stream(stream, request, on_progress, on_stream, on_approval)
}

pub fn agent_turn_on_stream(
    stream: UnixStream,
    request: ClientRequest,
    on_approval: impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
) -> Result<ClientResponse, ClientError> {
    agent_turn_with_events_on_stream(stream, request, |_| {}, |_| {}, on_approval)
}

pub fn agent_turn_on_stream_with_callbacks(
    stream: UnixStream,
    request: ClientRequest,
    callbacks: AgentTurnCallbacks<
        impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
        impl FnMut(ToolApprovalPrompt) -> ToolApprovalDecision,
        impl FnMut(HumanTaskExecutionPrompt) -> Option<aibe_protocol::HumanTaskResult>,
    >,
) -> Result<ClientResponse, ClientError> {
    agent_turn_with_client_tools_on_stream(stream, request, |_| {}, |_| {}, |_| None, callbacks)
}

pub fn agent_turn_with_client_tools(
    socket_path: &Path,
    request: ClientRequest,
    on_progress: impl FnMut(AgentTurnProgressEvent),
    on_stream: impl FnMut(String),
    on_client_tool: impl FnMut(ClientToolCallRequest) -> Option<ClientToolResult>,
    callbacks: AgentTurnCallbacks<
        impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
        impl FnMut(ToolApprovalPrompt) -> ToolApprovalDecision,
        impl FnMut(HumanTaskExecutionPrompt) -> Option<aibe_protocol::HumanTaskResult>,
    >,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    agent_turn_with_client_tools_on_stream(
        stream,
        request,
        on_progress,
        on_stream,
        on_client_tool,
        callbacks,
    )
}

/// `agent_turn` を送り、承認 prompt があれば `on_approval` で応答する。
pub fn agent_turn(
    socket_path: &std::path::Path,
    request: ClientRequest,
    on_approval: impl FnMut(ShellExecApprovalPrompt) -> ShellExecApprovalDecision,
) -> Result<ClientResponse, ClientError> {
    let stream = connect_unix_stream(socket_path, CONNECT_TIMEOUT).map_err(ClientError::Connect)?;
    agent_turn_on_stream(stream, request, on_approval)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::thread;

    #[test]
    fn send_request_serializes_ping() {
        let (mut client, mut server) = UnixStream::pair().expect("pair");
        send_request(&mut client, &ClientRequest::Ping { id: "p1".into() }).expect("send");
        let mut line = String::new();
        BufReader::new(&mut server)
            .read_line(&mut line)
            .expect("read");
        assert!(line.contains(r#""type":"ping""#));
    }

    #[test]
    fn client_tool_roundtrip_sends_request_and_result() {
        let (client, server) = UnixStream::pair().expect("pair");
        let handle = thread::spawn(move || {
            let mut writer = server.try_clone().expect("clone");
            let mut reader = BufReader::new(server);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read request");
            let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
            match req {
                ClientRequest::AgentTurn { client_tools, .. } => {
                    assert_eq!(client_tools.len(), 1);
                }
                other => panic!("unexpected request: {other:?}"),
            }
            let prompt = ClientResponse::ClientToolCallRequested {
                id: "prompt-1".into(),
                turn_id: "turn-1".into(),
                call_id: "call-1".into(),
                name: "aish.replay_show".into(),
                arguments: serde_json::json!({"index": 1}),
            };
            writeln!(
                writer,
                "{}",
                serde_json::to_string(&prompt).expect("serialize prompt")
            )
            .expect("write prompt");
            writer.flush().expect("flush prompt");

            line.clear();
            reader
                .read_line(&mut line)
                .expect("read client tool result");
            let result_req: ClientRequest =
                serde_json::from_str(line.trim()).expect("parse result");
            match result_req {
                ClientRequest::ClientToolResult(result) => {
                    assert_eq!(result.call_id, "call-1");
                    assert_eq!(result.status, ClientToolResultStatus::Ok);
                }
                other => panic!("unexpected result request: {other:?}"),
            }

            let final_resp = ClientResponse::AgentTurnResult {
                id: "turn-1".into(),
                status: aibe_protocol::AgentTurnStatus::Ok,
                assistant_message: aibe_protocol::ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "done".into(),
                },
                tool_calls: vec![],
            };
            writeln!(
                writer,
                "{}",
                serde_json::to_string(&final_resp).expect("serialize final")
            )
            .expect("write final");
            writer.flush().expect("flush final");
        });

        let request = ClientRequest::AgentTurn {
            id: "turn-1".into(),
            messages: vec![aibe_protocol::ProtocolMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: vec![],
            client_tools: vec![aibe_protocol::ClientProvidedToolSpec {
                name: "aish.replay_show".into(),
                description: "show".into(),
                parameters: serde_json::json!({"type":"object"}),
                risk_class: aibe_protocol::ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            }],
            context: aibe_protocol::RequestContext {
                cwd: Some("/tmp".into()),
                ..Default::default()
            },
            llm_profile: None,
        };
        let result = agent_turn_with_client_tools_on_stream(
            client,
            request,
            |_| {},
            |_| {},
            |call| {
                assert_eq!(call.name, "aish.replay_show");
                Some(ClientToolResult {
                    id: call.id,
                    turn_id: call.turn_id,
                    call_id: call.call_id,
                    status: ClientToolResultStatus::Ok,
                    error_kind: None,
                    content: "[untrusted terminal output]\nindex: 1\n".into(),
                })
            },
            shell_exec_only_callbacks(|_| ShellExecApprovalDecision {
                approved: true,
                approval_origin: ShellExecApprovalOrigin::UiYes,
                handoff_result: None,
                handoff_error: None,
            }),
        )
        .expect("agent turn");

        handle.join().expect("server thread");
        match result {
            ClientResponse::AgentTurnResult { .. } => {}
            other => panic!("expected prompt response, got {other:?}"),
        }
    }

    #[test]
    fn approval_decision_invariant_rejects_contradictions() {
        use aibe_protocol::{
            HandoffExecutionOutcome, HumanHandoffFailure, HumanHandoffResult,
            RequestedCommandCompletion, ShellExecApprovalOrigin,
        };

        let valid_normal = ShellExecApprovalDecision {
            approved: true,
            approval_origin: ShellExecApprovalOrigin::UiYes,
            handoff_result: None,
            handoff_error: None,
        };
        validate_shell_exec_approval_decision(&valid_normal).expect("normal approval");

        let sample_handoff_result = HumanHandoffResult {
            execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
            requested_command: None,
            requested_command_completion: RequestedCommandCompletion::Unknown,
            human_shell_exit_code: None,
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
        };
        let valid_handoff = ShellExecApprovalDecision {
            approved: true,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: Some(sample_handoff_result.clone()),
            handoff_error: None,
        };
        validate_shell_exec_approval_decision(&valid_handoff).expect("handoff success");

        let both_set = ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: Some(sample_handoff_result.clone()),
            handoff_error: Some(HumanHandoffFailure {
                code: "human_handoff_failed".into(),
                message: "fail".into(),
            }),
        };
        assert!(validate_shell_exec_approval_decision(&both_set).is_err());

        let collaborative_approved_without_result = ShellExecApprovalDecision {
            approved: true,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: None,
            handoff_error: None,
        };
        assert!(
            validate_shell_exec_approval_decision(&collaborative_approved_without_result).is_err()
        );

        let collaborative_denied_without_error = ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: None,
            handoff_error: None,
        };
        assert!(
            validate_shell_exec_approval_decision(&collaborative_denied_without_error).is_err()
        );

        let collaborative_success_requires_approved = ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: Some(sample_handoff_result.clone()),
            handoff_error: None,
        };
        assert!(
            validate_shell_exec_approval_decision(&collaborative_success_requires_approved)
                .is_err()
        );

        let collaborative_failure_requires_denied = ShellExecApprovalDecision {
            approved: true,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: None,
            handoff_error: Some(HumanHandoffFailure {
                code: "human_handoff_failed".into(),
                message: "fail".into(),
            }),
        };
        assert!(
            validate_shell_exec_approval_decision(&collaborative_failure_requires_denied).is_err()
        );

        let normal_approval_rejects_handoff_fields = ShellExecApprovalDecision {
            approved: true,
            approval_origin: ShellExecApprovalOrigin::UiYes,
            handoff_result: Some(sample_handoff_result.clone()),
            handoff_error: None,
        };
        assert!(
            validate_shell_exec_approval_decision(&normal_approval_rejects_handoff_fields).is_err()
        );

        let empty_handoff_error_code = ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
            handoff_result: None,
            handoff_error: Some(HumanHandoffFailure {
                code: String::new(),
                message: "fail".into(),
            }),
        };
        assert!(validate_shell_exec_approval_decision(&empty_handoff_error_code).is_err());
    }
}
