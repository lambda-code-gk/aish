//! `RequestService` の protocol 入口検証（0004: ToolName 変換・0003/0005: cwd 優先・context 正規化）。

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::MockLlm;
use aibe::application::tool_round::ToolRoundExecutor;
use aibe::application::RequestService;
use aibe::ports::outbound::{TerminationCapability, ToolsConfig};
use aibe::protocol::{ClientRequest, ClientResponse, ErrorCode, ProtocolMessage, RequestContext};

fn service() -> RequestService {
    let tools_config = ToolsConfig::default();
    let llm: Arc<dyn aibe::ports::outbound::LlmProvider> = Arc::new(MockLlm::new());
    let registry = build_registry(&tools_config);
    let executor = ToolRoundExecutor::new(Arc::clone(&llm), registry, tools_config.clone());
    RequestService::new(
        llm,
        executor,
        Arc::new(ToolRoundTerminatorOrchestrator::new(
            tools_config.termination_strategy,
        )),
        TerminationCapability::summary_prompt_only(),
    )
}

fn user_hi() -> Vec<ProtocolMessage> {
    vec![ProtocolMessage {
        role: "user".into(),
        content: "hi".into(),
    }]
}

#[tokio::test]
async fn unknown_tool_rejected_at_protocol_entry() {
    let res = service()
        .handle(ClientRequest::AgentTurn {
            id: "1".into(),
            messages: user_hi(),
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
            messages: user_hi(),
            tools: vec!["nope".into(), "read_file".into()],
            context: RequestContext::default(),
        })
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidRequest),
        _ => panic!("expected invalid_request before tool_not_allowed"),
    }
}

#[tokio::test]
async fn text_only_without_cwd_is_ok() {
    let res = service()
        .handle(ClientRequest::AgentTurn {
            id: "1".into(),
            messages: user_hi(),
            tools: vec![],
            context: RequestContext::default(),
        })
        .await;
    match res {
        ClientResponse::AgentTurnResult { .. } => {}
        other => panic!("expected ok for text-only without cwd: {other:?}"),
    }
}

#[tokio::test]
async fn empty_shell_log_tail_does_not_inject_prefix() {
    let res = service()
        .handle(ClientRequest::AgentTurn {
            id: "1".into(),
            messages: user_hi(),
            tools: vec![],
            context: RequestContext {
                shell_log_tail: Some("".into()),
                ..Default::default()
            },
        })
        .await;
    match res {
        ClientResponse::AgentTurnResult { .. } => {}
        other => panic!("expected ok: {other:?}"),
    }
}
