#![cfg(feature = "memory")]
//! Capability model boundary tests（§15.7）。

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::{
    shared_baseline_recipe_loader, shared_builtin_loader, ConversationStore,
    EmptyContextualMemoryStore, FilesystemMemorySpaceResolver, InProcessMemorySubscriptionBroker,
    MockLlm, StaticCapabilityPolicy,
};
use aibe::application::memory_service::MemoryService;
use aibe::application::memory_subscribe_service::MemorySubscribeService;
use aibe::application::RequestService;
use aibe::application::{basic_pack_arc, contextual_pack_arc};
use aibe::domain::FeatureRegistry;
use aibe::domain::{LlmStepResult, ToolCall, SHELL_EXEC};
use aibe::ports::outbound::{ProfileRegistry, ToolsConfig};
use aibe_protocol::{
    ClientRequest, ClientResponse, MemoryContext, MemoryOperationAdd, MemoryOperationArchive,
    MemoryOperationDto, MemoryQueryDto, MemoryQueryRequestBody, MemoryScopeDto,
    MemorySubscribeRequestBody, ProtocolMessage, RequestContext,
};
use serde_json::json;
use tempfile::tempdir;

fn request_service_with_policy(
    policy: Arc<dyn aibe::ports::outbound::CapabilityPolicy>,
) -> RequestService {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let profile_registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let tool_registry = build_registry(&tools_config, &[]);
    let (rpc_extension, turn_hook) = contextual_pack_arc(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        Arc::new(InProcessMemorySubscriptionBroker::new()),
        Arc::clone(&policy),
        profile_registry.clone(),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    RequestService::new(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".to_string(),
        Arc::new(aibe::adapters::outbound::ConversationStore::new(
            std::env::temp_dir().join("aibe-test-capability"),
        )),
        policy,
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    )
}

fn memory_service_with_policy(
    policy: Arc<dyn aibe::ports::outbound::CapabilityPolicy>,
) -> MemoryService {
    MemoryService::with_capability_policy(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        None,
        policy,
    )
}

fn valid_session() -> String {
    "01234567890123456789012345678901".to_string()
}

fn memory_context() -> MemoryContext {
    MemoryContext {
        cwd: Some(
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        ),
        memory_space_id: Some("test-space".into()),
    }
}

#[test]
fn memory_read_required_for_query() {
    let service = memory_service_with_policy(StaticCapabilityPolicy::memory_read_only());
    let res = service.query(
        "q1".into(),
        valid_session(),
        &memory_context(),
        MemoryQueryDto {
            kind: None,
            scope: None,
            status: None,
            active_only: false,
            include_archived: false,
            limit: None,
            include_prompt_block: false,
            user_query: None,
        },
    );
    assert!(matches!(res, ClientResponse::MemoryQueryResult { .. }));

    let service = memory_service_with_policy(Arc::new(StaticCapabilityPolicy::new("no_read", [])));
    let res = service.query(
        "q2".into(),
        valid_session(),
        &memory_context(),
        MemoryQueryDto {
            kind: None,
            scope: None,
            status: None,
            active_only: false,
            include_archived: false,
            limit: None,
            include_prompt_block: false,
            user_query: None,
        },
    );
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:read")),
        other => panic!("expected capability error: {other:?}"),
    }
}

#[test]
fn memory_write_required_for_add() {
    let service = memory_service_with_policy(StaticCapabilityPolicy::memory_read_only());
    let op = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "goal".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: None,
        status: None,
        text: "x".into(),
        make_active: None,
    });
    let res = service.apply("a1".into(), valid_session(), &memory_context(), op);
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:write")),
        other => panic!("expected write denied: {other:?}"),
    }
}

#[test]
fn memory_archive_required_for_clear_kind() {
    use aibe_protocol::{MemoryOperationClearKind, MemoryStatusDto};

    let service = memory_service_with_policy(StaticCapabilityPolicy::memory_read_only());
    let op = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
        kind: "goal".into(),
        scope: MemoryScopeDto::Project,
    });
    let res = service.apply("a2".into(), valid_session(), &memory_context(), op);
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:archive")),
        other => panic!("expected archive denied: {other:?}"),
    }

    let service = memory_service_with_policy(StaticCapabilityPolicy::local_full());
    let add = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "goal".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: None,
        status: Some(MemoryStatusDto::Active),
        text: "x".into(),
        make_active: Some(true),
    });
    let _ = service.apply("seed".into(), valid_session(), &memory_context(), add);
    let clear = MemoryOperationDto::ClearKind(MemoryOperationClearKind {
        kind: "goal".into(),
        scope: MemoryScopeDto::Project,
    });
    let res = service.apply("a3".into(), valid_session(), &memory_context(), clear);
    assert!(matches!(res, ClientResponse::MemoryApplyResult { .. }));
}

#[test]
fn memory_archive_required_for_archive_operation() {
    use aibe::adapters::outbound::FilesystemContextualMemoryStore;
    use aibe_protocol::MemoryStatusDto;

    let dir = tempdir().expect("tempdir");
    let store = Arc::new(FilesystemContextualMemoryStore::new(
        dir.path().to_path_buf(),
    ));
    let resolver = Arc::new(FilesystemMemorySpaceResolver);
    let full_service = MemoryService::with_capability_policy(
        store.clone(),
        resolver.clone(),
        shared_builtin_loader(),
        None,
        StaticCapabilityPolicy::local_full(),
    );

    let add = MemoryOperationDto::Add(MemoryOperationAdd {
        kind: "note".into(),
        scope: Some(MemoryScopeDto::Project),
        inject: None,
        status: Some(MemoryStatusDto::Open),
        text: "archive me".into(),
        make_active: None,
    });
    let apply_res = full_service.apply("seed".into(), valid_session(), &memory_context(), add);
    let entry_id = match apply_res {
        ClientResponse::MemoryApplyResult { entries, .. } => entries[0].id.clone(),
        other => panic!("expected apply ok: {other:?}"),
    };

    let archive = MemoryOperationDto::Archive(MemoryOperationArchive {
        id: entry_id.clone(),
        expected_version: None,
    });
    let read_only_service = MemoryService::with_capability_policy(
        store.clone(),
        resolver.clone(),
        shared_builtin_loader(),
        None,
        StaticCapabilityPolicy::memory_read_only(),
    );
    let denied = read_only_service.apply(
        "arch-deny".into(),
        valid_session(),
        &memory_context(),
        archive.clone(),
    );
    match denied {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:archive")),
        other => panic!("expected archive denied: {other:?}"),
    }

    let allowed = full_service.apply(
        "arch-ok".into(),
        valid_session(),
        &memory_context(),
        archive,
    );
    assert!(matches!(allowed, ClientResponse::MemoryApplyResult { .. }));
}

#[test]
fn memory_subscribe_requires_memory_subscribe_capability() {
    let broker: Arc<dyn aibe::ports::outbound::MemorySubscriptionBroker> =
        Arc::new(InProcessMemorySubscriptionBroker::new());
    let resolver: Arc<dyn aibe::ports::outbound::MemorySpaceResolver> =
        Arc::new(FilesystemMemorySpaceResolver);
    let allowed = MemorySubscribeService::with_capability_policy(
        Arc::clone(&broker),
        Arc::clone(&resolver),
        StaticCapabilityPolicy::local_full(),
    );
    let (res, sub) = allowed.begin(MemorySubscribeRequestBody {
        id: "sub-ok".into(),
        session_id: valid_session(),
        context: memory_context(),
        kind: None,
    });
    assert!(matches!(res, ClientResponse::MemorySubscribeResult { .. }));
    assert!(sub.is_some());

    let denied = MemorySubscribeService::with_capability_policy(
        broker,
        resolver,
        Arc::new(StaticCapabilityPolicy::new("no_subscribe", [])),
    );
    let (res, sub) = denied.begin(MemorySubscribeRequestBody {
        id: "sub-deny".into(),
        session_id: valid_session(),
        context: memory_context(),
        kind: None,
    });
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:subscribe")),
        other => panic!("expected subscribe denied: {other:?}"),
    }
    assert!(sub.is_none());
}

#[tokio::test]
async fn memory_query_rpc_requires_memory_read() {
    let service = request_service_with_policy(Arc::new(StaticCapabilityPolicy::new("no_read", [])));
    let res = service
        .handle(
            ClientRequest::MemoryQuery(MemoryQueryRequestBody {
                id: "mq1".into(),
                session_id: valid_session(),
                context: memory_context(),
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
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:read")),
        other => panic!("expected error: {other:?}"),
    }
}

#[tokio::test]
async fn shell_execute_is_independent_from_memory_capabilities() {
    let dir = tempdir().expect("tempdir");
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "echo", "args": ["cap-test"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(aibe::adapters::outbound::ScriptedMockLlm::new(steps));
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.shell_exec.enabled = true;
    tools_cfg.shell_exec.approval = aibe::ports::outbound::ShellExecApprovalMode::Never;
    let strategy = tools_cfg.termination_strategy;
    let profile_registry = ProfileRegistry::single(
        "default",
        llm,
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let tool_registry = build_registry(&tools_cfg, &[]);
    let policy = StaticCapabilityPolicy::memory_read_only();
    let (rpc_extension, turn_hook) = contextual_pack_arc(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        Arc::new(InProcessMemorySubscriptionBroker::new()),
        Arc::clone(&policy),
        profile_registry.clone(),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    let service = RequestService::new(
        profile_registry,
        tool_registry,
        tools_cfg,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".to_string(),
        Arc::new(aibe::adapters::outbound::ConversationStore::new(
            dir.path().join("conversations"),
        )),
        policy,
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    );

    let cwd = std::env::current_dir().expect("cwd");
    let res = service
        .handle(
            ClientRequest::AgentTurn {
                id: "turn1".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "run echo".into(),
                }],
                tools: vec![SHELL_EXEC.into()],
                context: RequestContext {
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            assert!(tool_calls.iter().any(|t| t.name.as_str() == SHELL_EXEC));
        }
        other => panic!("shell should work with memory_read_only: {other:?}"),
    }
}

#[tokio::test]
async fn memory_only_profile_denies_shell_execute() {
    let dir = tempdir().expect("tempdir");
    let steps = vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                name: SHELL_EXEC.to_string(),
                arguments: json!({"command": "echo", "args": ["blocked"]}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("done"),
    ];
    let llm = Arc::new(aibe::adapters::outbound::ScriptedMockLlm::new(steps));
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.shell_exec.enabled = true;
    tools_cfg.shell_exec.approval = aibe::ports::outbound::ShellExecApprovalMode::Never;
    let strategy = tools_cfg.termination_strategy;
    let profile_registry = ProfileRegistry::single(
        "default",
        llm,
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let tool_registry = build_registry(&tools_cfg, &[]);
    let policy = StaticCapabilityPolicy::memory_only();
    let (rpc_extension, turn_hook) = contextual_pack_arc(
        Arc::new(EmptyContextualMemoryStore),
        Arc::new(FilesystemMemorySpaceResolver),
        shared_builtin_loader(),
        shared_baseline_recipe_loader(),
        Arc::new(InProcessMemorySubscriptionBroker::new()),
        Arc::clone(&policy),
        profile_registry.clone(),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    let service = RequestService::new(
        profile_registry,
        tool_registry,
        tools_cfg,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".to_string(),
        Arc::new(aibe::adapters::outbound::ConversationStore::new(
            dir.path().join("conversations"),
        )),
        policy,
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    );

    let cwd = std::env::current_dir().expect("cwd");
    let res = service
        .handle(
            ClientRequest::AgentTurn {
                id: "turn2".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "run echo".into(),
                }],
                tools: vec![SHELL_EXEC.into()],
                context: RequestContext {
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    ..Default::default()
                },
                llm_profile: None,
            },
            None,
        )
        .await;
    match res {
        ClientResponse::AgentTurnResult { tool_calls, .. } => {
            let shell = tool_calls
                .iter()
                .find(|t| t.name.as_str() == SHELL_EXEC)
                .expect("shell_exec call");
            assert_eq!(shell.status, aibe_protocol::ExecutedToolStatus::Error);
            assert!(shell
                .message
                .as_deref()
                .unwrap_or("")
                .contains("shell:execute"));
        }
        other => panic!("expected agent turn with tool error: {other:?}"),
    }
}

#[tokio::test]
async fn memory_recipe_run_requires_memory_recipe_run_capability() {
    let dir = tempdir().expect("tempdir");
    let store: Arc<dyn aibe::ports::outbound::ContextualMemoryStore> = Arc::new(
        aibe::adapters::outbound::FilesystemContextualMemoryStore::new(dir.path().to_path_buf()),
    );
    let profile_registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let service =
        aibe::application::memory_recipe_service::MemoryRecipeService::with_capability_policy(
            store,
            Arc::new(FilesystemMemorySpaceResolver),
            shared_builtin_loader(),
            shared_baseline_recipe_loader(),
            profile_registry,
            None,
            Arc::new(StaticCapabilityPolicy::new(
                "no_recipe",
                [
                    aibe::domain::Capability::MemoryRead,
                    aibe::domain::Capability::MemoryWrite,
                ],
            )),
            Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
        );
    let res = service
        .run(
            "r0".into(),
            valid_session(),
            &memory_context(),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:recipe_run")),
        other => panic!("expected recipe_run denied: {other:?}"),
    }
}

#[tokio::test]
async fn memory_recipe_run_requires_memory_read() {
    let dir = tempdir().expect("tempdir");
    let store: Arc<dyn aibe::ports::outbound::ContextualMemoryStore> = Arc::new(
        aibe::adapters::outbound::FilesystemContextualMemoryStore::new(dir.path().to_path_buf()),
    );
    let profile_registry = ProfileRegistry::single(
        "default",
        Arc::new(MockLlm::new()),
        aibe::ports::outbound::TerminationCapability::summary_prompt_only(),
    );
    let service =
        aibe::application::memory_recipe_service::MemoryRecipeService::with_capability_policy(
            store,
            Arc::new(FilesystemMemorySpaceResolver),
            shared_builtin_loader(),
            shared_baseline_recipe_loader(),
            profile_registry,
            None,
            Arc::new(StaticCapabilityPolicy::new(
                "recipe_no_read",
                [aibe::domain::Capability::MemoryRecipeRun],
            )),
            Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
        );
    let res = service
        .run(
            "r1".into(),
            valid_session(),
            &memory_context(),
            "clarify-goal",
            false,
            None,
        )
        .await;
    match res {
        ClientResponse::Error { message, .. } => assert!(message.contains("memory:read")),
        other => panic!("expected read denied: {other:?}"),
    }
}

#[tokio::test]
async fn local_full_allows_text_only_agent_turn() {
    let service = request_service_with_policy(StaticCapabilityPolicy::local_full());
    let res = service
        .handle(
            ClientRequest::AgentTurn {
                id: "turn3".into(),
                messages: vec![ProtocolMessage {
                    role: "user".into(),
                    content: "hi".into(),
                }],
                tools: vec![],
                context: RequestContext::default(),
                llm_profile: None,
            },
            None,
        )
        .await;
    assert!(matches!(res, ClientResponse::AgentTurnResult { .. }));
}
