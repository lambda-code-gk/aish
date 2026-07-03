#![cfg(feature = "memory")]
//! `RequestService` の protocol 入口検証（0004: ToolName 変換・0003/0005: cwd 優先・context 正規化）。

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::{
    shared_baseline_recipe_loader, shared_builtin_loader, ConversationStore,
    EmptyContextualMemoryStore, FilesystemMemorySpaceResolver, InProcessMemorySubscriptionBroker,
    MockLlm, StaticCapabilityPolicy,
};
use aibe::application::build_default_tool_registry;
use aibe::application::contextual_pack_arc;
use aibe::application::RequestService;
use aibe::domain::FeatureRegistry;
use aibe::ports::outbound::{ProfileRegistry, ToolsConfig};
use aibe_protocol::{
    ClientProvidedToolSpec, ClientRequest, ClientResponse, ErrorCode, ProtocolMessage,
    RequestContext, ToolRiskClass,
};

fn service() -> RequestService {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let profile_registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let tool_registry = build_default_tool_registry(&tools_config, &[]);
    let (rpc_extension, turn_hook) = contextual_pack_arc(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        Arc::new(InProcessMemorySubscriptionBroker::new()),
        StaticCapabilityPolicy::local_full(),
        profile_registry.clone(),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    RequestService::new(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".to_string(),
        Arc::new(ConversationStore::new(
            std::env::temp_dir().join("aibe-test-conversations"),
        )),
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    )
}

fn user_hi() -> Vec<ProtocolMessage> {
    vec![ProtocolMessage {
        role: "user".into(),
        content: "hi".into(),
    }]
}

#[tokio::test]
async fn unknown_message_role_rejected_at_protocol_entry() {
    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "1".into(),
                messages: vec![ProtocolMessage {
                    role: "moderator".into(),
                    content: "hi".into(),
                }],
                tools: vec![],
                client_tools: vec![],
                context: RequestContext::default(),
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::Error { code, message, .. } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert!(message.contains("unknown message role"));
        }
        _ => panic!("expected invalid_request for unknown role"),
    }
}

#[tokio::test]
async fn unknown_tool_rejected_at_protocol_entry() {
    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "1".into(),
                messages: user_hi(),
                tools: vec!["nope".into()],
                client_tools: vec![],
                context: RequestContext {
                    cwd: Some("/tmp".into()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::ToolNotAllowed),
        _ => panic!("expected tool_not_allowed"),
    }
}

#[tokio::test]
async fn missing_cwd_takes_priority_over_unknown_tool() {
    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "1".into(),
                messages: user_hi(),
                tools: vec!["nope".into(), "read_file".into()],
                client_tools: vec![],
                context: RequestContext::default(),
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidRequest),
        _ => panic!("expected invalid_request before tool_not_allowed"),
    }
}

#[tokio::test]
async fn text_only_without_cwd_is_ok() {
    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "1".into(),
                messages: user_hi(),
                tools: vec![],
                client_tools: vec![],
                context: RequestContext::default(),
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { .. } => {}
        other => panic!("expected ok for text-only without cwd: {other:?}"),
    }
}

#[tokio::test]
async fn empty_shell_log_tail_does_not_inject_prefix() {
    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "1".into(),
                messages: user_hi(),
                tools: vec![],
                client_tools: vec![],
                context: RequestContext {
                    shell_log_tail: Some("".into()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { .. } => {}
        other => panic!("expected ok: {other:?}"),
    }
}

#[tokio::test]
async fn aibe_does_not_read_aish_session_dir_for_client_tools() {
    // 重要: 本テスト中 AISH_SESSION_DIR を読みに行く I/O は走らない。
    // aibe は client_tools を受けても session dir をオープンせず、
    // turn は read-only client tool のみで完結する。
    let prev = std::env::var("AISH_SESSION_DIR").ok();
    std::env::remove_var("AISH_SESSION_DIR");

    let res = service()
        .handle(
            ClientRequest::AgentTurn {
                id: "turn-1".into(),
                messages: user_hi(),
                tools: vec![],
                client_tools: vec![ClientProvidedToolSpec {
                    name: "aish.replay_show".into(),
                    description: "show".into(),
                    parameters: serde_json::json!({"type":"object"}),
                    risk_class: ToolRiskClass::ReadOnly,
                    max_output_bytes: 8192,
                }],
                context: RequestContext {
                    cwd: Some("/tmp".into()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;

    if let Some(v) = prev {
        std::env::set_var("AISH_SESSION_DIR", v);
    }

    match res {
        ClientResponse::AgentTurnResult { .. } => {}
        other => panic!("expected agent turn result: {other:?}"),
    }
}
