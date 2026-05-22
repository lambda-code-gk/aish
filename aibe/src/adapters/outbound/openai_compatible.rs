//! OpenAI 互換 HTTP API アダプタ。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::ChatMessage;
use crate::ports::outbound::{LlmError, LlmProvider};

pub struct OpenAiCompatibleLlm {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleLlm {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key,
            model,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages: messages
                .iter()
                .map(|m| ApiMessage {
                    role: m.role.clone(),
                    content: m.content.clone(),
                })
                .collect(),
        };

        let response = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?;

        if !status.is_success() {
            return Err(LlmError::Provider(format!("HTTP {status}: {text}")));
        }

        let parsed: ChatResponse = serde_json::from_str(&text)
            .map_err(|e| LlmError::Provider(format!("invalid response JSON: {e}; body: {text}")))?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .map(|m| m.content)
            .unwrap_or_default();

        Ok(ChatMessage::assistant(content))
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
}

#[derive(Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<ApiMessage>,
}
