//! contextual memory 無効化（Phase A）の境界テスト。

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::{ConversationStore, MockLlm, ScriptedMockLlm};
use aibe::application::basic_pack_arc;
use aibe::application::memory_runtime::MEMORY_DISABLED_MESSAGE;
use aibe::application::RequestService;
use aibe::domain::FeatureRegistry;
use aibe::domain::LlmStepResult;
use aibe::ports::outbound::{ProfileRegistry, TerminationCapability, ToolsConfig};
use aibe_protocol::{
    ClientRequest, ClientResponse, MemoryContext, MemoryKindListRequestBody, MemoryOperationAdd,
    MemoryOperationDto, MemoryQueryDto, MemoryQueryRequestBody, MemoryRecipeRunRequestBody,
    ProtocolMessage, RequestContext, RouteTurnCliOverrides, RouteTurnConversation,
    RouteTurnSession,
};

fn memory_disabled_service() -> RequestService {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let profile_registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let tool_registry = build_registry(&tools_config, &[]);
    let (rpc_extension, turn_hook) = basic_pack_arc();
    RequestService::new_with_turns_and_packs(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        "default".to_string(),
        Arc::new(ConversationStore::new(
            std::env::temp_dir().join("aibe-test-memory-disabled"),
        )),
        aibe::adapters::outbound::StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
        aibe::domain::FeatureEligibilityContext::default(),
    )
}

fn session_id() -> String {
    "01234567890123456789012345678901".to_string()
}

#[tokio::test]
async fn memory_apply_rejected_when_disabled() {
    let service = memory_disabled_service();
    let resp = service
        .handle(
            ClientRequest::MemoryApply(aibe_protocol::MemoryApplyRequestBody {
                id: "m1".into(),
                session_id: session_id(),
                context: MemoryContext {
                    cwd: None,
                    memory_space_id: None,
                },
                operation: MemoryOperationDto::Add(MemoryOperationAdd {
                    kind: "goal".into(),
                    scope: None,
                    inject: None,
                    status: None,
                    text: "x".into(),
                    make_active: None,
                }),
            }),
            None,
        )
        .await;
    match resp {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains(MEMORY_DISABLED_MESSAGE));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn memory_query_rejected_when_disabled() {
    let service = memory_disabled_service();
    let resp = service
        .handle(
            ClientRequest::MemoryQuery(MemoryQueryRequestBody {
                id: "q1".into(),
                session_id: session_id(),
                context: MemoryContext {
                    cwd: None,
                    memory_space_id: None,
                },
                query: MemoryQueryDto {
                    kind: None,
                    scope: None,
                    status: None,
                    active_only: false,
                    include_archived: false,
                    limit: None,
                    include_prompt_block: false,
                    user_query: None,
                },
            }),
            None,
        )
        .await;
    match resp {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains(MEMORY_DISABLED_MESSAGE));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn agent_turn_skips_memory_injection_when_disabled() {
    let service = memory_disabled_service();
    let resp = service
        .handle(
            ClientRequest::AgentTurn {
                id: "t1".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "hello".into(),
                }],
                tools: vec![],
                context: RequestContext {
                    ai_session_id: Some(session_id()),
                    memory_space_id: Some("ctx_a".into()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    match resp {
        ClientResponse::AgentTurnResult {
            assistant_message, ..
        } => {
            assert_eq!(assistant_message.content, "[mock] received: hello");
        }
        other => panic!("expected agent turn: {other:?}"),
    }
}

fn memory_context() -> MemoryContext {
    MemoryContext {
        cwd: None,
        memory_space_id: None,
    }
}

fn assert_memory_disabled_error(resp: ClientResponse) {
    match resp {
        ClientResponse::Error { message, .. } => {
            assert!(message.contains(MEMORY_DISABLED_MESSAGE));
        }
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn memory_kind_list_rejected_when_disabled() {
    let service = memory_disabled_service();
    let resp = service
        .handle(
            ClientRequest::MemoryKindList(MemoryKindListRequestBody {
                id: "kl1".into(),
                session_id: session_id(),
                context: memory_context(),
            }),
            None,
        )
        .await;
    assert_memory_disabled_error(resp);
}

#[tokio::test]
async fn memory_recipe_run_rejected_when_disabled() {
    let service = memory_disabled_service();
    let resp = service
        .handle(
            ClientRequest::MemoryRecipeRun(MemoryRecipeRunRequestBody {
                id: "r1".into(),
                session_id: session_id(),
                context: memory_context(),
                recipe: "goal".into(),
                apply: false,
                user_instruction: None,
            }),
            None,
        )
        .await;
    assert_memory_disabled_error(resp);
}

#[tokio::test]
async fn server_starts_with_broken_kinds_toml_when_memory_disabled() {
    use std::io::Write;
    use std::time::Duration;

    use aibe::application::server;
    use aibe::ports::outbound::TerminationCapability;
    use aibe::ports::outbound::{MemoryConfig, ProfileRegistry, ToolsConfig};
    use tempfile::tempdir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let dir = tempdir().expect("tempdir");
    let store_root = dir.path().join("data");
    std::fs::create_dir_all(&store_root).expect("mkdir");
    let kinds_path = dir.path().join("memory/kinds.toml");
    std::fs::create_dir_all(kinds_path.parent().unwrap()).expect("mkdir memory");
    let mut f = std::fs::File::create(&kinds_path).expect("create kinds.toml");
    f.write_all(b"not valid toml [[[\n")
        .expect("write broken toml");

    let socket_path = dir.path().join("aibe.sock");
    let socket_for_server = socket_path.clone();
    let llm = Arc::new(MockLlm::new());
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let server = tokio::spawn(async move {
        server::run(
            socket_for_server,
            profile_registry,
            ToolsConfig::default(),
            Vec::new(),
            "default".to_string(),
            store_root,
            MemoryConfig::disabled(),
        )
        .await
        .expect("server should start with memory disabled despite broken kinds.toml");
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&socket_path).await.expect("connect");
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let req = r#"{"type":"ping","id":"p1"}"#;
    writer.write_all(req.as_bytes()).await.expect("write");
    writer.write_all(b"\n").await.expect("newline");
    let line = lines.next_line().await.expect("read").expect("line");
    assert!(line.contains(r#""type":"pong""#), "pong: {line}");

    server.abort();
}

#[tokio::test]
async fn route_turn_strips_feature_actions_when_feature_registry_empty() {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let llm = Arc::new(ScriptedMockLlm::new(vec![LlmStepResult::text_only(
        r#"{"route_kind":"tool_assisted","new_conversation":false,"recommended_tools":["read_file"],"feature_actions":[{"type":"memory_query","query":{}}],"route_reason":"inspect error"}"#
            .to_string(),
    )]));
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let tool_registry = build_registry(&tools_config, &[]);
    let (rpc_extension, turn_hook) = basic_pack_arc();
    let service = RequestService::new_with_turns_and_packs(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        "default".to_string(),
        Arc::new(ConversationStore::new(
            std::env::temp_dir().join("aibe-test-memory-disabled-route"),
        )),
        aibe::adapters::outbound::StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
        aibe::domain::FeatureEligibilityContext::default(),
    );
    let resp = service
        .handle(
            ClientRequest::RouteTurn {
                id: "route-disabled".into(),
                query: "直近のエラーを調べて".into(),
                cwd: "/tmp/proj".into(),
                session: RouteTurnSession {
                    ai_session_id: session_id(),
                    aish_session_dir: None,
                    tty: true,
                },
                conversation: RouteTurnConversation {
                    conversation_id: None,
                    recent_summary: None,
                    new_conversation: true,
                    preprocessor_hints: None,
                },
                cli_overrides: RouteTurnCliOverrides::default(),
            },
            None,
        )
        .await;
    match resp {
        ClientResponse::RouteTurnResult { plan, .. } => {
            assert!(
                plan.feature_actions.is_empty(),
                "empty registry must not return feature_actions: {:?}",
                plan.feature_actions
            );
        }
        other => panic!("expected route_turn_result: {other:?}"),
    }
}
