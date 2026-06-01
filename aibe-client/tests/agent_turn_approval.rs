//! `agent_turn_on_stream` の承認 prompt → approval → final 往復。

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::thread;

use aibe_client::{agent_turn_on_stream, ShellExecApprovalPrompt};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessage, ProtocolMessageOut,
};

const PROMPT_ID: &str = "approval-prompt-1";
const TURN_ID: &str = "turn-approval-test";
const TOOL_CALL_ID: &str = "call_exec_1";
const COMMAND: &str = "echo";
const ARGS: &[&str] = &["hi"];

fn run_mock_server(mut server: UnixStream, expect_approved: bool) {
    let mut reader = BufReader::new(server.try_clone().expect("clone"));
    let mut req_line = String::new();
    reader.read_line(&mut req_line).expect("read agent_turn");
    let req: ClientRequest = serde_json::from_str(req_line.trim()).expect("parse request");
    let ClientRequest::AgentTurn { id, .. } = req else {
        panic!("expected agent_turn request");
    };
    assert_eq!(id, TURN_ID);

    let prompt = ClientResponse::ShellExecApprovalPrompt {
        id: PROMPT_ID.into(),
        turn_id: TURN_ID.into(),
        tool_call_id: TOOL_CALL_ID.into(),
        command: COMMAND.into(),
        args: ARGS.iter().map(|s| (*s).to_string()).collect(),
    };
    let payload = serde_json::to_string(&prompt).expect("serialize prompt");
    writeln!(server, "{payload}").expect("write prompt");
    server.flush().expect("flush prompt");

    let mut approval_line = String::new();
    reader.read_line(&mut approval_line).expect("read approval");
    let approval: ClientRequest =
        serde_json::from_str(approval_line.trim()).expect("parse approval");
    let ClientRequest::ShellExecApproval {
        id,
        turn_id,
        tool_call_id,
        approved,
    } = approval
    else {
        panic!("expected shell_exec_approval");
    };
    assert_eq!(id, PROMPT_ID);
    assert_eq!(turn_id, TURN_ID);
    assert_eq!(tool_call_id, TOOL_CALL_ID);
    assert_eq!(approved, expect_approved);

    let final_resp = ClientResponse::AgentTurnResult {
        id: TURN_ID.into(),
        status: AgentTurnStatus::Ok,
        assistant_message: ProtocolMessageOut {
            role: "assistant".into(),
            content: if expect_approved {
                "approved path".into()
            } else {
                "denied path".into()
            },
        },
        tool_calls: vec![],
    };
    let payload = serde_json::to_string(&final_resp).expect("serialize final");
    writeln!(server, "{payload}").expect("write final");
    server.flush().expect("flush final");
}

fn agent_turn_request() -> ClientRequest {
    ClientRequest::AgentTurn {
        id: TURN_ID.into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: "run".into(),
        }],
        tools: vec!["shell_exec".into()],
        context: Default::default(),
        llm_profile: None,
    }
}

#[test]
fn agent_turn_approval_roundtrip_approved() {
    let (client, server) = UnixStream::pair().expect("pair");
    let handle = thread::spawn(move || run_mock_server(server, true));

    let mut seen_prompt = false;
    let resp = agent_turn_on_stream(client, agent_turn_request(), |p| {
        seen_prompt = true;
        assert_eq!(p.prompt_id, PROMPT_ID);
        assert_eq!(p.turn_id, TURN_ID);
        assert_eq!(p.tool_call_id, TOOL_CALL_ID);
        assert_eq!(p.command, COMMAND);
        assert_eq!(p.args, ARGS);
        true
    })
    .expect("agent_turn");

    handle.join().expect("server thread");
    assert!(seen_prompt);
    match resp {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => assert_eq!(assistant_message.content, "approved path"),
        other => panic!("expected agent_turn_result, got {other:?}"),
    }
}

#[test]
fn agent_turn_approval_roundtrip_denied() {
    let (client, server) = UnixStream::pair().expect("pair");
    let handle = thread::spawn(move || run_mock_server(server, false));

    let resp = agent_turn_on_stream(
        client,
        agent_turn_request(),
        |_p: ShellExecApprovalPrompt| false,
    )
    .expect("agent_turn");

    handle.join().expect("server thread");
    match resp {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => assert_eq!(assistant_message.content, "denied path"),
        other => panic!("expected agent_turn_result, got {other:?}"),
    }
}
