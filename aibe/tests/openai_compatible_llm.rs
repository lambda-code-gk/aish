#![cfg(unix)]

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::OpenAiCompatibleLlm;
use aibe::application::agent_turn::AgentTurnService;
use aibe::domain::{AgentTurnContext, ChatMessage, ClientCwd, ToolName};
use aibe::ports::outbound::{LlmError, LlmProvider, TerminationCapability, ToolsConfig};
use aibe::protocol::{ClientResponse, ErrorCode};
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
async fn complete_with_tools_rejects_unknown_tool_from_model() {
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
    let err = llm
        .complete_with_tools(
            &[ChatMessage::user("do it")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .unwrap_err();

    assert_eq!(err, LlmError::UnknownTool("delete_everything".into()));
}

#[tokio::test]
async fn agent_turn_unknown_tool_from_llm_returns_tool_not_allowed() {
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
    let llm = Arc::new(OpenAiCompatibleLlm::new(
        base,
        "test-key".to_string(),
        "test-model".to_string(),
    ));
    let cfg = ToolsConfig::default();
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        cfg.termination_strategy,
    ));
    let svc = AgentTurnService::new(
        llm,
        build_registry(&cfg),
        cfg,
        terminator,
        TerminationCapability::summary_prompt_only(),
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
        )
        .await;

    match res {
        ClientResponse::Error { code, message, .. } => {
            assert_eq!(code, ErrorCode::ToolNotAllowed);
            assert_eq!(message, "unknown tool: delete_everything");
        }
        other => panic!("expected tool_not_allowed, got {other:?}"),
    }
}
