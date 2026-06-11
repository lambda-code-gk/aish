#![cfg(unix)]

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::{EmptyContextualMemoryStore, OpenAiCompatibleLlm};
use aibe::application::agent_turn::AgentTurnService;
use aibe::application::tool_round::ToolRoundExecutor;
use aibe::domain::{AgentTurnContext, ChatMessage, ClientCwd, ExecutedToolStatus, ToolName};
use aibe::ports::outbound::{LlmProvider, TerminationCapability, ToolsConfig};
use aibe_protocol::{AgentTurnStatus, ClientResponse};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn openai_compatible_calls_chat_completions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": { "role": "assistant", "content": "ok from mock http" }
            }]
        })))
        .mount(&server)
        .await;

    let base = format!("{}/v1", server.uri());
    let llm = OpenAiCompatibleLlm::new(base, "test-key".to_string(), "test-model".to_string());
    let out = llm
        .complete(&[ChatMessage::user("hi")])
        .await
        .expect("complete");
    assert_eq!(out.content, "ok from mock http");
}

#[tokio::test]
async fn complete_with_tools_accepts_unknown_tool_name_from_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_bad",
                        "type": "function",
                        "function": {
                            "name": "delete_everything",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        })))
        .mount(&server)
        .await;

    let base = format!("{}/v1", server.uri());
    let llm = OpenAiCompatibleLlm::new(base, "test-key".to_string(), "test-model".to_string());
    let step = llm
        .complete_with_tools(
            &[ChatMessage::user("do it")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("parse unknown tool name without failing");

    assert_eq!(step.tool_calls.len(), 1);
    assert_eq!(step.tool_calls[0].name, "delete_everything");
}

#[tokio::test]
async fn agent_turn_unknown_tool_from_llm_returns_tool_result_and_continues() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_bad",
                        "type": "function",
                        "function": {
                            "name": "delete_everything",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": { "role": "assistant", "content": "cannot delete, explained" }
            }]
        })))
        .mount(&server)
        .await;

    let base = format!("{}/v1", server.uri());
    let llm: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatibleLlm::new(
        base,
        "test-key".to_string(),
        "test-model".to_string(),
    ));
    let cfg = ToolsConfig::default();
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        cfg.termination_strategy,
    ));
    let registry = build_registry(&cfg, &[]);
    let executor = ToolRoundExecutor::new(Arc::clone(&llm), registry, cfg.clone());
    let svc = AgentTurnService::new(
        llm,
        executor,
        terminator,
        TerminationCapability::summary_prompt_only(),
        Arc::new(EmptyContextualMemoryStore),
    );
    let res = svc
        .run(
            "turn-unknown-tool".into(),
            vec![ChatMessage::user("clean disk")],
            vec![ToolName::read_file()],
            AgentTurnContext::for_tool_turn(
                ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"),
                None,
            ),
            None,
        )
        .await;

    match res {
        ClientResponse::AgentTurnResult {
            status,
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(status, AgentTurnStatus::Ok);
            assert_eq!(assistant_message.content, "cannot delete, explained");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].status, ExecutedToolStatus::Error);
            assert_eq!(tool_calls[0].error.as_deref(), Some("tool_not_implemented"));
            assert!(tool_calls[0]
                .message
                .as_deref()
                .unwrap_or("")
                .contains("unknown tool: delete_everything"));
        }
        other => panic!("expected agent_turn_result, got {other:?}"),
    }
}
