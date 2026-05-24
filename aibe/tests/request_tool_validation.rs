//! `RequestService` の protocol 入口検証（0004: ToolName 変換・0003: cwd 優先）。

use std::sync::Arc;

use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::MockLlm;
use aibe::application::RequestService;
use aibe::ports::outbound::ToolsConfig;
use aibe::protocol::{ClientRequest, ClientResponse, ErrorCode, ProtocolMessage, RequestContext};

fn service() -> RequestService {
    RequestService::new(
        Arc::new(MockLlm::new()),
        build_registry(&ToolsConfig::default()),
        ToolsConfig::default(),
    )
}

#[tokio::test]
async fn unknown_tool_rejected_at_protocol_entry() {
    let res = service()
        .handle(ClientRequest::AgentTurn {
            id: "1".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: vec!["nope".into()],
            context: RequestContext {
                cwd: Some("/tmp".into()),
                ..Default::default()
            },
        })
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::ToolNotAllowed),
        _ => panic!("expected tool_not_allowed"),
    }
}

#[tokio::test]
async fn missing_cwd_takes_priority_over_unknown_tool() {
    let res = service()
        .handle(ClientRequest::AgentTurn {
            id: "1".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: vec!["nope".into(), "read_file".into()],
            context: RequestContext::default(),
        })
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidRequest),
        _ => panic!("expected invalid_request before tool_not_allowed"),
    }
}
