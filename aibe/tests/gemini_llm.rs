#![cfg(unix)]

use std::sync::Arc;

use aibe::adapters::outbound::terminator::ToolRoundTerminatorOrchestrator;
use aibe::adapters::outbound::tools::build_registry;
use aibe::adapters::outbound::{GeminiLlm, StaticCapabilityPolicy};
use aibe::application::agent_turn::AgentTurnService;
use aibe::application::basic_pack_arc;
use aibe::application::tool_round::ToolRoundExecutor;
use aibe::domain::{
    AgentTurnContext, ChatMessage, ClientCwd, MessageRole, ToolCall, ToolName,
    AISH_REPLAY_SHOW_LOGICAL,
};
use aibe::ports::outbound::{LlmProvider, TerminationCapability, ToolsConfig};
use aibe_protocol::ClientResponse;
use serde_json::{json, Value};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn gemini_llm(server: &MockServer) -> GeminiLlm {
    GeminiLlm::new(
        format!("{}/v1beta", server.uri()),
        "test-key".to_string(),
        "test-model".to_string(),
    )
}

fn success_body(parts: Value) -> Value {
    json!({
        "candidates": [{
            "content": { "parts": parts }
        }]
    })
}

#[tokio::test]
async fn gemini_complete_calls_generate_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "ok from mock http" }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let out = llm
        .complete(&[ChatMessage::user("hi")])
        .await
        .expect("complete");
    assert_eq!(out.content, "ok from mock http");
}

#[tokio::test]
async fn complete_with_tools_sends_function_declarations() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "done" }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let _ = llm
        .complete_with_tools(
            &[ChatMessage::user("read")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("complete_with_tools");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json body");
    assert!(body.get("tools").is_some());
    assert!(body.get("toolConfig").is_some());
    assert!(body["tools"][0]["functionDeclarations"]
        .as_array()
        .is_some_and(|a| !a.is_empty()));
    let declaration = &body["tools"][0]["functionDeclarations"][0];
    assert!(declaration.get("parameters").is_none());
    assert_eq!(
        declaration["parametersJsonSchema"]["properties"]["path"]["type"],
        "string"
    );
}

#[tokio::test]
async fn complete_with_client_tool_preserves_additional_properties_in_json_schema() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "done" }
            ]))),
        )
        .mount(&server)
        .await;

    let tool = aibe::application::client_tool_defs::canonical_aish_replay_show_tool_definition();
    let llm = gemini_llm(&server);
    let _ = llm
        .complete_with_tools(&[ChatMessage::user("show replay")], &[tool])
        .await
        .expect("complete_with_tools");

    let requests = server.received_requests().await.expect("requests");
    assert_eq!(requests.len(), 1);
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json body");
    let declaration = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(declaration["name"], "aish_replay_show");
    assert!(declaration.get("parameters").is_none());
    assert_eq!(
        declaration["parametersJsonSchema"]["additionalProperties"],
        false
    );
}

#[tokio::test]
async fn client_tool_function_response_uses_same_provider_name_as_function_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "done" }
            ]))),
        )
        .mount(&server)
        .await;

    let tool = aibe::application::client_tool_defs::canonical_aish_replay_show_tool_definition();
    let messages = vec![
        ChatMessage::user("show replay"),
        ChatMessage::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "call_0_0".into(),
                name: AISH_REPLAY_SHOW_LOGICAL.into(),
                arguments: json!({ "index": 1 }),
                provider_extras: Some(json!({ "thoughtSignature": "sig-replay" })),
            }],
        ),
        ChatMessage::tool("call_0_0", "recorded output"),
    ];

    let llm = gemini_llm(&server);
    let _ = llm
        .complete_with_tools(&messages, &[tool])
        .await
        .expect("complete_with_tools");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json body");
    assert_eq!(
        body["contents"][1]["parts"][0]["functionCall"]["name"],
        "aish_replay_show"
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["name"],
        "aish_replay_show"
    );
    assert_eq!(
        body["contents"][2]["parts"][0]["functionResponse"]["id"],
        "call_0_0"
    );
}

#[tokio::test]
async fn complete_omits_tools_and_tool_config() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "plain" }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let _ = llm
        .complete(&[ChatMessage::user("hi")])
        .await
        .expect("complete");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json body");
    assert!(body.get("tools").is_none());
    assert!(body.get("toolConfig").is_none());
}

#[tokio::test]
async fn parse_multiple_function_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "functionCall": { "id": "a", "name": "read_file", "args": { "path": "a.md" } } },
                { "functionCall": { "id": "b", "name": "read_file", "args": { "path": "b.md" } } }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let step = llm
        .complete_with_tools(
            &[ChatMessage::user("read both")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("step");

    assert_eq!(step.tool_calls.len(), 2);
    assert_eq!(step.tool_calls[0].id, "a");
    assert_eq!(step.tool_calls[1].id, "b");
}

#[tokio::test]
async fn synthetic_id_when_function_call_id_missing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "functionCall": { "name": "read_file", "args": { "path": "x.md" } } }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let step = llm
        .complete_with_tools(
            &[ChatMessage::user("read")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("step");

    assert_eq!(step.tool_calls.len(), 1);
    assert_eq!(step.tool_calls[0].id, "call_0_0");
}

#[tokio::test]
async fn provider_extras_preserves_thought_signature_on_resend() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(|req: &Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            let contents = body["contents"].as_array().cloned().unwrap_or_default();
            if contents.len() <= 1 {
                ResponseTemplate::new(200).set_body_json(success_body(json!([{
                    "functionCall": {
                        "id": "call_0_0",
                        "name": "read_file",
                        "args": { "path": "a.md" }
                    },
                    "thoughtSignature": "sig-round-1"
                }])))
            } else {
                let model_parts = &contents[1]["parts"];
                assert_eq!(
                    model_parts[0].get("thoughtSignature"),
                    Some(&json!("sig-round-1"))
                );
                ResponseTemplate::new(200).set_body_json(success_body(json!([
                    { "text": "finished" }
                ])))
            }
        })
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let step1 = llm
        .complete_with_tools(
            &[ChatMessage::user("read")],
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("round1");

    let mut conversation = vec![ChatMessage::user("read")];
    conversation.push(ChatMessage::assistant_with_tools(
        step1.assistant.content,
        step1.tool_calls,
    ));
    conversation.push(ChatMessage::tool("call_0_0", "file body"));

    let final_msg = llm
        .complete_with_tools(
            &conversation,
            &[aibe::application::tool_defs::definitions_for(&[ToolName::read_file()])[0].clone()],
        )
        .await
        .expect("round2");
    assert_eq!(final_msg.assistant.content, "finished");
}

#[tokio::test]
async fn complete_with_tools_accepts_unknown_tool_name_from_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "functionCall": {
                    "id": "call_bad",
                    "name": "delete_everything",
                    "args": {}
                }}
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
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
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "functionCall": {
                    "id": "call_bad",
                    "name": "delete_everything",
                    "args": {}
                }}
            ]))),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body(
            json!([{ "text": "cannot delete, explained" }]),
        )))
        .mount(&server)
        .await;

    let llm: Arc<dyn LlmProvider> = Arc::new(gemini_llm(&server));
    let cfg = ToolsConfig::default();
    let terminator = Arc::new(ToolRoundTerminatorOrchestrator::new(
        cfg.termination_strategy,
    ));
    let registry = build_registry(&cfg, &[]);
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
    let res = svc
        .run(
            "turn-unknown-tool-gemini".into(),
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
            assistant_message,
            tool_calls,
            ..
        } => {
            assert_eq!(assistant_message.content, "cannot delete, explained");
            assert_eq!(tool_calls.len(), 1);
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

#[tokio::test]
async fn system_messages_become_system_instruction() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/v1beta/models/test-model:generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(success_body(json!([
                { "text": "ok" }
            ]))),
        )
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let _ = llm
        .complete(&[
            ChatMessage {
                role: MessageRole::System,
                content: "rule a".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: MessageRole::System,
                content: "rule b".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage::user("hi"),
        ])
        .await
        .expect("complete");

    let requests = server.received_requests().await.expect("requests");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json");
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"],
        "rule a\n\nrule b"
    );
    let roles: Vec<_> = body["contents"]
        .as_array()
        .expect("contents")
        .iter()
        .filter_map(|c| c.get("role").and_then(|r| r.as_str()))
        .collect();
    assert!(!roles.contains(&"system"));
}

#[tokio::test]
async fn complete_streaming_parses_crlf_sse() {
    let server = MockServer::start().await;
    let sse_body = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hello\"}]}}]}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]}}]}\r\n\r\n",
    );
    Mock::given(method("POST"))
        .and(path_regex(
            r"/v1beta/models/test-model:streamGenerateContent",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(sse_body))
        .mount(&server)
        .await;

    let llm = gemini_llm(&server);
    let mut deltas = Vec::new();
    let out = llm
        .complete_streaming(&[ChatMessage::user("hi")], &mut |delta| deltas.push(delta))
        .await
        .expect("complete_streaming");
    assert_eq!(out.content, "hello world");
    assert_eq!(deltas.join(""), "hello world");
}
