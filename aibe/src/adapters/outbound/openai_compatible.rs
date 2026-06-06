//! OpenAI 互換 HTTP API アダプタ。

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::outbound::llm_backend::HttpBackendContext;
use crate::domain::{ChatMessage, LlmStepResult, ToolCall};
use crate::ports::outbound::{LlmError, LlmGenerationParams, LlmProvider, ToolDefinition};

pub struct OpenAiCompatibleLlm {
    backend: Arc<HttpBackendContext>,
    model: String,
    params: LlmGenerationParams,
}

impl OpenAiCompatibleLlm {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        Self::with_backend(
            HttpBackendContext::new(base_url, api_key),
            model,
            LlmGenerationParams::default(),
        )
    }

    pub fn with_backend(
        backend: Arc<HttpBackendContext>,
        model: String,
        params: LlmGenerationParams,
    ) -> Self {
        Self {
            backend,
            model,
            params,
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.backend.base_url)
    }

    async fn chat_completion(&self, body: ChatRequest) -> Result<ChatResponse, LlmError> {
        let response = self
            .backend
            .client
            .post(self.chat_url())
            .bearer_auth(&self.backend.api_key)
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

        serde_json::from_str(&text)
            .map_err(|e| LlmError::Provider(format!("invalid response JSON: {e}; body: {text}")))
    }

    async fn chat_completion_streaming(
        &self,
        mut body: ChatRequest,
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ChatResponse, LlmError> {
        body.stream = Some(true);
        let response = self
            .backend
            .client
            .post(self.chat_url())
            .bearer_auth(&self.backend.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .map_err(|e| LlmError::Provider(e.to_string()))?;
            return Err(LlmError::Provider(format!("HTTP {status}: {text}")));
        }

        let mut response = response;
        let mut buffer = String::new();
        let mut text = String::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| LlmError::Provider(e.to_string()))?
        {
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(split) = buffer.find("\n\n") {
                let event = buffer[..split].to_string();
                buffer.drain(..split + 2);
                for line in event.lines() {
                    let Some(data) = line.strip_prefix("data:") else {
                        continue;
                    };
                    let payload = data.trim();
                    if payload == "[DONE]" {
                        return Ok(ChatResponse {
                            choices: vec![Choice {
                                message: Some(ApiMessage {
                                    role: "assistant".to_string(),
                                    content: Some(text),
                                    tool_call_id: None,
                                    tool_calls: None,
                                }),
                            }],
                        });
                    }
                    let chunk: ChatChunk = serde_json::from_str(payload).map_err(|e| {
                        LlmError::Provider(format!("invalid stream JSON: {e}; body: {payload}"))
                    })?;
                    if let Some(choice) = chunk.choices.into_iter().next() {
                        if let Some(delta) = choice.delta.content {
                            if !delta.is_empty() {
                                text.push_str(&delta);
                                on_delta(delta);
                            }
                        }
                    }
                }
            }
        }

        Ok(ChatResponse {
            choices: vec![Choice {
                message: Some(ApiMessage {
                    role: "assistant".to_string(),
                    content: Some(text),
                    tool_call_id: None,
                    tool_calls: None,
                }),
            }],
        })
    }
}

fn to_api_messages(messages: &[ChatMessage]) -> Vec<ApiMessage> {
    messages
        .iter()
        .map(|m| {
            let mut api = ApiMessage {
                role: m.role.to_string(),
                content: Some(m.content.clone()),
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: None,
            };
            if let Some(calls) = &m.tool_calls {
                api.tool_calls = Some(
                    calls
                        .iter()
                        .map(|tc| ApiToolCall {
                            id: tc.id.clone(),
                            r#type: "function".to_string(),
                            function: ApiFunctionCall {
                                name: tc.name.clone(),
                                arguments: tc.arguments.to_string(),
                            },
                        })
                        .collect(),
                );
                if m.content.is_empty() {
                    api.content = None;
                }
            }
            api
        })
        .collect()
}

fn parse_tool_calls(message: &ApiMessage) -> Result<Vec<ToolCall>, LlmError> {
    let Some(calls) = message.tool_calls.as_ref() else {
        return Ok(vec![]);
    };

    calls
        .iter()
        .map(|c| {
            let args: Value = serde_json::from_str(&c.function.arguments).unwrap_or(Value::Null);
            Ok(ToolCall {
                id: c.id.clone(),
                name: c.function.name.clone(),
                arguments: args,
                provider_extras: None,
            })
        })
        .collect()
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages: to_api_messages(messages),
            tools: None,
            temperature: self.params.temperature,
            max_tokens: self.params.max_output_tokens,
            stream: None,
        };
        let parsed = self.chat_completion(body).await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .unwrap_or(ApiMessage {
                role: "assistant".to_string(),
                content: Some(String::new()),
                tool_call_id: None,
                tool_calls: None,
            });
        Ok(ChatMessage::assistant(message.content.unwrap_or_default()))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let api_tools: Vec<ApiTool> = tools
            .iter()
            .map(|t| ApiTool {
                r#type: "function".to_string(),
                function: ApiToolFunction {
                    name: t.name.as_str().to_string(),
                    description: t.description.clone(),
                    parameters: t.parameters.clone(),
                },
            })
            .collect();

        let body = ChatRequest {
            model: self.model.clone(),
            messages: to_api_messages(messages),
            tools: if api_tools.is_empty() {
                None
            } else {
                Some(api_tools)
            },
            temperature: self.params.temperature,
            max_tokens: self.params.max_output_tokens,
            stream: None,
        };

        let parsed = self.chat_completion(body).await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .unwrap_or(ApiMessage {
                role: "assistant".to_string(),
                content: Some(String::new()),
                tool_call_id: None,
                tool_calls: None,
            });

        let tool_calls = parse_tool_calls(&message)?;
        let content = message.content.unwrap_or_default();
        Ok(LlmStepResult::with_tool_calls(content, tool_calls))
    }

    async fn complete_streaming(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ChatMessage, LlmError> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages: to_api_messages(messages),
            tools: None,
            temperature: self.params.temperature,
            max_tokens: self.params.max_output_tokens,
            stream: Some(true),
        };
        let parsed = self.chat_completion_streaming(body, on_delta).await?;
        let message = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message)
            .unwrap_or(ApiMessage {
                role: "assistant".to_string(),
                content: Some(String::new()),
                tool_call_id: None,
                tool_calls: None,
            });
        Ok(ChatMessage::assistant(message.content.unwrap_or_default()))
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct ApiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall>>,
}

#[derive(Serialize)]
struct ApiTool {
    r#type: String,
    function: ApiToolFunction,
}

#[derive(Serialize)]
struct ApiToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize, Deserialize)]
struct ApiToolCall {
    id: String,
    r#type: String,
    function: ApiFunctionCall,
}

#[derive(Serialize, Deserialize)]
struct ApiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Option<ApiMessage>,
}

#[derive(Deserialize)]
struct ChatChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    delta: ChunkDelta,
}

#[derive(Deserialize)]
struct ChunkDelta {
    #[serde(default)]
    content: Option<String>,
}
