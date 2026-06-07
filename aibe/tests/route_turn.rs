//! `route_turn` の routing と conversation store 回帰。

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm};
use aibe::application::RequestService;
use aibe::domain::LlmStepResult;
use aibe::ports::outbound::{ProfileRegistry, TerminationCapability, ToolsConfig};
use aibe_protocol::{
    ClientRequest, ClientResponse, RouteKind, RouteTurnCliOverrides, RouteTurnConversation,
    RouteTurnSession,
};
use tempfile::tempdir;

fn route_response_json() -> String {
    r#"{"route_kind":"continue","new_conversation":false,"recommended_preset":"fast","recommended_tools":["read_file"],"log_tail_bytes":128,"require_shell_approval":true,"log_tail_escalation":false,"route_reason":"continue using /tmp/secret/log","confidence":0.8}"#
        .to_string()
}

fn service(store_root: std::path::PathBuf) -> RequestService {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::text_only(route_response_json()),
        LlmStepResult::text_only(route_response_json()),
        LlmStepResult::text_only(route_response_json()),
    ]));
    let profile_registry =
        ProfileRegistry::single("fast", llm, TerminationCapability::summary_prompt_only());
    let tool_registry = build_registry(&tools_config, &[]);
    RequestService::new(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "fast".to_string(),
        Arc::new(ConversationStore::new(store_root)),
    )
}

fn route_request(
    id: &str,
    session_id: &str,
    new_conversation: bool,
    conversation_id: Option<&str>,
    recent_summary: Option<&str>,
) -> ClientRequest {
    ClientRequest::RouteTurn {
        id: id.to_string(),
        query: "hello".into(),
        cwd: "/tmp/proj".into(),
        session: RouteTurnSession {
            ai_session_id: session_id.into(),
            aish_session_dir: Some("/tmp/aish".into()),
            tty: true,
        },
        conversation: RouteTurnConversation {
            conversation_id: conversation_id.map(str::to_string),
            recent_summary: recent_summary.map(str::to_string),
            new_conversation,
        },
        cli_overrides: RouteTurnCliOverrides {
            preset: Some("fast".into()),
            tools: Some(vec!["read_file".into()]),
            log_tail_bytes: Some(128),
            yes_exec: true,
        },
    }
}

#[tokio::test]
async fn route_turn_saves_redacted_plan_and_reuses_latest_conversation() {
    let dir = tempdir().expect("tempdir");
    let store_root = dir.path().join("conversations");
    let service = service(store_root.clone());

    let first = service
        .handle(
            route_request(
                "route-1",
                "session-1",
                false,
                None,
                Some("user: hello | assistant: world"),
            ),
            None,
        )
        .await;
    let plan_1 = match first {
        ClientResponse::RouteTurnResult { plan, .. } => plan,
        other => panic!("expected route_turn_result, got {other:?}"),
    };
    assert_eq!(plan_1.route_kind, RouteKind::Continue);
    assert_eq!(plan_1.recommended_preset.as_deref(), Some("fast"));
    assert!(plan_1.require_shell_approval);

    let store = ConversationStore::new(store_root.clone());
    let snapshot = store
        .load_snapshot("session-1", &plan_1.conversation_id)
        .expect("load snapshot")
        .expect("snapshot");
    assert_eq!(
        snapshot.route_plan.as_ref().map(|p| p.route_kind),
        Some(RouteKind::Continue)
    );
    assert_eq!(
        snapshot.summary.as_deref(),
        Some("user: hello | assistant: world")
    );
    assert!(!snapshot
        .route_plan
        .as_ref()
        .expect("route plan")
        .route_reason
        .contains("/tmp/secret"));

    let index_raw =
        std::fs::read_to_string(store_root.join("session-1").join("index.jsonl")).expect("index");
    assert!(!index_raw.contains("/tmp/secret"));

    let second = service
        .handle(
            route_request("route-2", "session-1", false, None, None),
            None,
        )
        .await;
    let plan_2 = match second {
        ClientResponse::RouteTurnResult { plan, .. } => plan,
        other => panic!("expected route_turn_result, got {other:?}"),
    };
    assert_eq!(plan_2.conversation_id, plan_1.conversation_id);

    let third = service
        .handle(
            route_request("route-3", "session-1", true, None, None),
            None,
        )
        .await;
    let plan_3 = match third {
        ClientResponse::RouteTurnResult { plan, .. } => plan,
        other => panic!("expected route_turn_result, got {other:?}"),
    };
    assert_ne!(plan_3.conversation_id, plan_1.conversation_id);
}
