use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm, StaticCapabilityPolicy};
use aibe::application::{basic_pack_arc, build_default_tool_registry, RequestService};
use aibe::domain::{ChatMessage, FeatureRegistry, LlmStepResult};
use aibe::ports::outbound::{
    LlmError, LlmProvider, ProfileRegistry, TerminationCapability, ToolDefinition, ToolsConfig,
};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, ProtocolMessage, RequestContext};
use async_trait::async_trait;

fn service_with_llm(llm: Arc<dyn LlmProvider>) -> RequestService {
    let tools_config = ToolsConfig::default();
    let strategy = tools_config.termination_strategy;
    let profile_registry =
        ProfileRegistry::single("default", llm, TerminationCapability::summary_prompt_only());
    let tool_registry = build_default_tool_registry(&tools_config, &[]);
    let (rpc_extension, turn_hook) = basic_pack_arc();
    RequestService::new(
        profile_registry,
        tool_registry,
        tools_config,
        Arc::new(ToolRoundTerminatorOrchestrator::new(strategy)),
        "default".into(),
        Arc::new(ConversationStore::new(
            std::env::temp_dir().join("aibe-test-0065-continuation"),
        )),
        StaticCapabilityPolicy::local_full(),
        rpc_extension,
        turn_hook,
        FeatureRegistry::empty(),
    )
}

fn request(id: &str) -> ClientRequest {
    ClientRequest::AgentTurn {
        id: id.into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: "continue".into(),
        }],
        tools: vec![],
        client_tools: vec![],
        context: RequestContext {
            continuation_turn: true,
            ..Default::default()
        },
        llm_profile: None,
    }
}

#[tokio::test]
async fn aibe_rejects_duplicate_continuation_turn_id() {
    let service = service_with_llm(Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::text_only("continued"),
        LlmStepResult::text_only("must not run"),
    ])));
    let first = service.handle(request("continuation-1"), None).await;
    assert!(matches!(first, ClientResponse::AgentTurnResult { .. }));

    let duplicate = service.handle(request("continuation-1"), None).await;
    match duplicate {
        ClientResponse::Error { code, message, .. } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert!(message.contains("duplicate agent turn id"));
        }
        other => panic!("expected duplicate rejection, got {other:?}"),
    }
}

#[tokio::test]
async fn aibe_allows_retry_after_failed_continuation_turn() {
    let service = service_with_llm(Arc::new(FailThenSucceedLlm::new()));
    let first = service.handle(request("continuation-retry"), None).await;
    assert!(
        matches!(first, ClientResponse::Error { .. }),
        "expected first attempt Error, got {first:?}"
    );
    let second = service.handle(request("continuation-retry"), None).await;
    assert!(
        matches!(second, ClientResponse::AgentTurnResult { .. }),
        "expected retry after ordinary failure to be allowed, got {second:?}"
    );
}

/// First `complete_with_tools` fails; subsequent calls succeed.
struct FailThenSucceedLlm {
    calls: AtomicUsize,
}

impl FailThenSucceedLlm {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for FailThenSucceedLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        self.complete_with_tools(messages, &[])
            .await
            .map(|step| step.assistant)
    }

    async fn complete_with_tools(
        &self,
        _messages: &[ChatMessage],
        _tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(LlmError::Provider("simulated provider failure".into()))
        } else {
            Ok(LlmStepResult::text_only("retry-ok"))
        }
    }
}
