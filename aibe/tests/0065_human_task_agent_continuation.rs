use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::{ConversationStore, ScriptedMockLlm, StaticCapabilityPolicy};
use aibe::application::{basic_pack_arc, build_default_tool_registry, RequestService};
use aibe::domain::{ChatMessage, FeatureRegistry, LlmStepResult, ToolCall, READ_FILE};
use aibe::ports::outbound::{
    LlmError, LlmProvider, ProfileRegistry, ReadFileConfig, TerminationCapability, ToolDefinition,
    ToolsConfig,
};
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ErrorCode, ProtocolMessage, RequestContext,
};
use async_trait::async_trait;
use serde_json::json;
use tempfile::tempdir;

fn service_with_llm(llm: Arc<dyn LlmProvider>, tools_config: ToolsConfig) -> RequestService {
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

fn request(id: &str, tools: Vec<&str>, cwd: Option<&str>) -> ClientRequest {
    ClientRequest::AgentTurn {
        id: id.into(),
        messages: vec![ProtocolMessage {
            role: "user".into(),
            content: "continue".into(),
        }],
        tools: tools.into_iter().map(str::to_owned).collect(),
        client_tools: vec![],
        context: RequestContext {
            continuation_turn: true,
            cwd: cwd.map(str::to_owned),
            ..Default::default()
        },
        llm_profile: None,
    }
}

#[tokio::test]
async fn aibe_rejects_duplicate_continuation_turn_id() {
    let service = service_with_llm(
        Arc::new(ScriptedMockLlm::new(vec![
            LlmStepResult::text_only("continued"),
            LlmStepResult::text_only("must not run"),
        ])),
        ToolsConfig::default(),
    );
    let first = service
        .handle(request("continuation-1", vec![], None), None)
        .await;
    assert!(matches!(
        first,
        ClientResponse::AgentTurnResult {
            status: AgentTurnStatus::Ok,
            ..
        }
    ));

    let duplicate = service
        .handle(request("continuation-1", vec![], None), None)
        .await;
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
    let service = service_with_llm(Arc::new(FailThenSucceedLlm::new()), ToolsConfig::default());
    let first = service
        .handle(request("continuation-retry", vec![], None), None)
        .await;
    assert!(
        matches!(first, ClientResponse::Error { .. }),
        "expected first attempt Error, got {first:?}"
    );
    let second = service
        .handle(request("continuation-retry", vec![], None), None)
        .await;
    assert!(
        matches!(
            second,
            ClientResponse::AgentTurnResult {
                status: AgentTurnStatus::Ok,
                ..
            }
        ),
        "expected retry after ordinary failure to be allowed, got {second:?}"
    );
}

#[tokio::test]
async fn aibe_allows_retry_after_max_tool_rounds_continuation() {
    let dir = tempdir().expect("tempdir");
    let file_path = dir.path().join("note.txt");
    std::fs::write(&file_path, "hello").expect("write");
    let tools_config = ToolsConfig {
        max_rounds: 0,
        read_file: ReadFileConfig {
            allowed_roots: vec![dir.path().to_path_buf()],
        },
        ..Default::default()
    };
    let llm = Arc::new(ScriptedMockLlm::new(vec![
        LlmStepResult::with_tool_calls(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: READ_FILE.to_string(),
                arguments: json!({"path": file_path}),
                provider_extras: None,
            }],
        ),
        LlmStepResult::text_only("retry after max rounds"),
    ]));
    let service = service_with_llm(llm, tools_config);
    let turn_id = "continuation-max-rounds";
    let cwd = dir.path().to_string_lossy();
    let first = service
        .handle(request(turn_id, vec![READ_FILE], Some(cwd.as_ref())), None)
        .await;
    match first {
        ClientResponse::AgentTurnResult {
            status: AgentTurnStatus::MaxToolRounds,
            ..
        } => {}
        other => panic!("expected MaxToolRounds, got {other:?}"),
    }
    let second = service
        .handle(request(turn_id, vec![], Some(cwd.as_ref())), None)
        .await;
    match second {
        ClientResponse::Error {
            code: ErrorCode::InvalidRequest,
            message,
            ..
        } if message.contains("duplicate agent turn id") => {
            panic!("MaxToolRounds must not permanently block the same continuation turn id")
        }
        ClientResponse::AgentTurnResult { .. } | ClientResponse::Error { .. } => {}
        other => panic!("unexpected second response: {other:?}"),
    }
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
