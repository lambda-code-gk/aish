// Phase 8 tests for 0054 Safe File Write Tools.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use aibe_client::{
    agent_turn_on_stream_with_callbacks, AgentTurnCallbacks, ShellExecApprovalDecision,
    ToolApprovalDecision,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessage, ProtocolMessageOut,
    ToolApprovalOrigin, ToolRiskClass, WRITE_FILE,
};

#[test]
fn aibe_client_tool_approval_roundtrip() {
    let (client, server) = UnixStream::pair().expect("pair");
    let handle = thread::spawn(|| run_mock_server(server));

    let mut seen_prompt = false;
    let resp = agent_turn_on_stream_with_callbacks(
        client,
        agent_turn_request(),
        AgentTurnCallbacks::new(
            |_| ShellExecApprovalDecision {
                approved: false,
                approval_origin: aibe_protocol::ShellExecApprovalOrigin::UiNo,
                handoff_result: None,
                handoff_error: None,
            },
            |prompt: aibe_client::ToolApprovalPrompt| {
                seen_prompt = true;
                assert_eq!(prompt.tool_name, WRITE_FILE);
                assert_eq!(prompt.preview, "+hello\n");
                ToolApprovalDecision::Approved(ToolApprovalOrigin::UiYes)
            },
        ),
    )
    .expect("agent turn");

    handle.join().expect("server thread");
    assert!(seen_prompt);
    match resp {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => assert_eq!(assistant_message.content, "done"),
        other => panic!("expected agent_turn_result, got {other:?}"),
    }
}

fn agent_turn_request() -> ClientRequest {
    ClientRequest::AgentTurn {
        id: "turn-tool-approval".into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: "write".into(),
        }],
        tools: vec![WRITE_FILE.into()],
        client_tools: vec![],
        context: Default::default(),
        llm_profile: None,
    }
}

fn run_mock_server(mut server: UnixStream) {
    let mut reader = BufReader::new(server.try_clone().expect("clone"));
    let mut line = String::new();
    reader.read_line(&mut line).expect("read request");
    let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
    let ClientRequest::AgentTurn { id, .. } = req else {
        panic!("expected agent_turn");
    };
    assert_eq!(id, "turn-tool-approval");

    let prompt = ClientResponse::ToolApprovalPrompt {
        id: "prompt-1".into(),
        turn_id: "turn-tool-approval".into(),
        tool_call_id: "call-write".into(),
        tool_name: WRITE_FILE.into(),
        risk_class: ToolRiskClass::WriteLike,
        summary: "create demo.txt (+1 -0, 0 -> 5 bytes)".into(),
        paths: vec!["demo.txt".into()],
        preview: "+hello\n".into(),
        preview_truncated: false,
    };
    writeln!(
        server,
        "{}",
        serde_json::to_string(&prompt).expect("serialize prompt")
    )
    .expect("write prompt");
    server.flush().expect("flush");

    line.clear();
    reader.read_line(&mut line).expect("read approval");
    let approval: ClientRequest = serde_json::from_str(line.trim()).expect("parse approval");
    let ClientRequest::ToolApproval {
        approved,
        approval_origin,
        tool_call_id,
        ..
    } = approval
    else {
        panic!("expected tool_approval");
    };
    assert_eq!(tool_call_id, "call-write");
    assert!(approved);
    assert_eq!(approval_origin, ToolApprovalOrigin::UiYes);

    let final_resp = ClientResponse::AgentTurnResult {
        id: "turn-tool-approval".into(),
        status: AgentTurnStatus::Ok,
        assistant_message: ProtocolMessageOut {
            role: "assistant".into(),
            content: "done".into(),
        },
        tool_calls: vec![],
        completion_report: None,
    };
    writeln!(
        server,
        "{}",
        serde_json::to_string(&final_resp).expect("serialize final")
    )
    .expect("write final");
    server.flush().expect("flush");
}
