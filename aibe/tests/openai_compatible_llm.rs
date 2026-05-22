#![cfg(unix)]

use aibe::adapters::outbound::OpenAiCompatibleLlm;
use aibe::domain::ChatMessage;
use aibe::ports::outbound::LlmProvider;
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
