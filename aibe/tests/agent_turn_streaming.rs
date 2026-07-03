//! assistant streaming の forward 検証。

use std::sync::{Arc, Mutex};

use aibe::adapters::outbound::{
    terminator::ToolRoundTerminatorOrchestrator, DeltaStreamingMockLlm, StaticCapabilityPolicy,
};
use aibe::application::agent_turn::AgentTurnService;
use aibe::application::basic_pack_arc;
use aibe::application::build_default_tool_registry;
use aibe::application::tool_round::ToolRoundExecutor;
use aibe::domain::{AgentTurnContext, ChatMessage};
use aibe::ports::outbound::{LlmProvider, TerminationCapability, ToolsConfig, TurnEventSink};
use aibe_protocol::{AgentTurnStatus, ClientResponse, ProgressPhase};

struct RecordingSink {
    deltas: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl TurnEventSink for RecordingSink {
    async fn progress(&self, _id: &str, _phase: ProgressPhase, _message: Option<String>) {}

    async fn assistant_streaming(&self, _id: &str, delta: String) {
        self.deltas.lock().expect("lock").push(delta);
    }

    async fn final_response(&self, _id: &str) {}
}

#[tokio::test]
async fn text_only_turn_forwards_multiple_streaming_deltas() {
    let llm: Arc<dyn LlmProvider> = Arc::new(DeltaStreamingMockLlm::new(
        vec!["hel".into(), "lo".into()],
        "hello",
    ));
    let cfg = ToolsConfig::default();
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        cfg.termination_strategy,
    ));
    let registry = build_default_tool_registry(&cfg, &[]);
    let executor = ToolRoundExecutor::new(
        Arc::clone(&llm),
        registry,
        cfg.clone(),
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    let (_, turn_hook) = basic_pack_arc();
    let svc = AgentTurnService::new(
        llm,
        executor,
        terminator,
        TerminationCapability::summary_prompt_only(),
        StaticCapabilityPolicy::local_full(),
        turn_hook,
        Arc::new(aibe::ports::outbound::NoopLlmCallTracer),
    );
    let sink = Arc::new(RecordingSink {
        deltas: Mutex::new(Vec::new()),
    });

    let res = svc
        .run_with_events(
            "stream-1".into(),
            vec![ChatMessage::user("hi")],
            vec![],
            AgentTurnContext::for_text_only(None),
            None,
            Some(Arc::clone(&sink) as Arc<dyn TurnEventSink>),
            None,
        )
        .await;

    match res {
        ClientResponse::AgentTurnResult {
            assistant_message,
            status,
            ..
        } => {
            assert_eq!(status, AgentTurnStatus::Ok);
            assert_eq!(assistant_message.content, "hello");
        }
        other => panic!("unexpected response: {other:?}"),
    }
    assert_eq!(sink.deltas.lock().expect("lock").as_slice(), &["hel", "lo"]);
}
