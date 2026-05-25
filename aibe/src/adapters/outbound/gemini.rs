//! Google AI Studio Gemini API (`generateContent` v1beta) アダプタ。

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::adapters::outbound::llm_backend::HttpBackendContext;
use crate::domain::{ChatMessage, LlmStepResult, MessageRole, ToolCall};
use crate::ports::outbound::{LlmError, LlmGenerationParams, LlmProvider, ToolDefinition};

pub struct GeminiLlm {
    backend: Arc<HttpBackendContext>,
    model: String,
    params: LlmGenerationParams,
}

impl GeminiLlm {
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

    fn generate_content_url(&self) -> String {
        format!(
            "{}/models/{}:generateContent",
            self.backend.base_url, self.model
        )
    }

    fn generation_config(&self) -> Option<GenerationConfig> {
        if self.params.temperature.is_none() && self.params.max_output_tokens.is_none() {
            return None;
        }
        Some(GenerationConfig {
            temperature: self.params.temperature,
            max_output_tokens: self.params.max_output_tokens,
        })
    }

    async fn generate_content(
        &self,
        body: GenerateContentRequest,
    ) -> Result<GeminiResponse, LlmError> {
        let response = self
            .backend
            .client
            .post(self.generate_content_url())
            .header("x-goog-api-key", &self.backend.api_key)
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
}

/// assistant（model）ターン数 — synthetic id の turn_index に使う。
fn model_turn_index(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .filter(|m| m.role == MessageRole::Assistant)
        .count()
}

fn synthetic_call_id(turn_index: usize, part_index: usize) -> String {
    format!("call_{turn_index}_{part_index}")
}

fn build_system_instruction(messages: &[ChatMessage]) -> Option<SystemInstruction> {
    let parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.clone())
        .collect();
    if parts.is_empty() {
        return None;
    }
    Some(SystemInstruction {
        parts: vec![TextPart {
            text: parts.join("\n\n"),
        }],
    })
}

fn messages_to_contents(messages: &[ChatMessage]) -> Vec<Content> {
    let mut contents = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        if msg.role == MessageRole::System {
            i += 1;
            continue;
        }
        if msg.role == MessageRole::Tool {
            let mut parts = Vec::new();
            while i < messages.len() && messages[i].role == MessageRole::Tool {
                let tool_msg = &messages[i];
                let call_id = tool_msg
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                let name = resolve_tool_name(messages, &call_id);
                parts.push(Part::FunctionResponse {
                    function_response: FunctionResponse {
                        id: call_id,
                        name,
                        response: json!({ "content": tool_msg.content }),
                    },
                });
                i += 1;
            }
            contents.push(Content {
                role: "user".to_string(),
                parts,
            });
            continue;
        }
        if msg.role == MessageRole::User {
            contents.push(Content {
                role: "user".to_string(),
                parts: vec![Part::Text {
                    text: msg.content.clone(),
                }],
            });
            i += 1;
            continue;
        }
        if msg.role == MessageRole::Assistant {
            let mut parts = Vec::new();
            if !msg.content.is_empty() {
                parts.push(Part::Text {
                    text: msg.content.clone(),
                });
            }
            if let Some(calls) = &msg.tool_calls {
                for tc in calls {
                    parts.push(tool_call_to_part(tc));
                }
            }
            contents.push(Content {
                role: "model".to_string(),
                parts,
            });
            i += 1;
            continue;
        }
        i += 1;
    }
    contents
}

fn resolve_tool_name(messages: &[ChatMessage], tool_call_id: &str) -> String {
    for msg in messages.iter().rev() {
        if let Some(calls) = &msg.tool_calls {
            for tc in calls {
                if tc.id == tool_call_id {
                    return tc.name.clone();
                }
            }
        }
    }
    "unknown_tool".to_string()
}

fn tool_call_to_part(tc: &ToolCall) -> Part {
    let mut fc = Map::new();
    fc.insert("id".into(), json!(tc.id));
    fc.insert("name".into(), json!(tc.name));
    fc.insert("args".into(), tc.arguments.clone());

    let mut part_obj = Map::new();
    part_obj.insert("functionCall".into(), Value::Object(fc));

    if let Some(extras) = &tc.provider_extras {
        if let Some(sig) = extras.get("thoughtSignature") {
            part_obj.insert("thoughtSignature".into(), sig.clone());
        }
    }

    Part::Raw(Value::Object(part_obj))
}

fn parse_candidate_parts(parts: &[Value], turn_index: usize) -> Result<LlmStepResult, LlmError> {
    let mut content_text = String::new();
    let mut tool_calls = Vec::new();
    let mut function_call_part_index = 0usize;

    for part in parts {
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            content_text.push_str(text);
            continue;
        }
        if let Some(fc) = part.get("functionCall") {
            let id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| synthetic_call_id(turn_index, function_call_part_index));
            let name_str = fc
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LlmError::Provider("functionCall missing name".into()))?;
            let args = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));

            let mut extras_map = Map::new();
            extras_map.insert("gemini_part_index".into(), json!(function_call_part_index));
            if let Some(sig) = part.get("thoughtSignature") {
                extras_map.insert("thoughtSignature".into(), sig.clone());
            }
            let provider_extras = Some(Value::Object(extras_map));

            tool_calls.push(ToolCall {
                id,
                name: name_str.to_string(),
                arguments: args,
                provider_extras,
            });
            function_call_part_index += 1;
        }
    }

    Ok(LlmStepResult::with_tool_calls(content_text, tool_calls))
}

fn validate_response(resp: &GeminiResponse) -> Result<&CandidateContent, LlmError> {
    if let Some(feedback) = &resp.prompt_feedback {
        if let Some(reason) = &feedback.block_reason {
            return Err(LlmError::Provider(format!("prompt blocked: {reason}")));
        }
    }

    let candidate = resp
        .candidates
        .first()
        .ok_or_else(|| LlmError::Provider("empty candidates".into()))?;

    if let Some(reason) = &candidate.finish_reason {
        let blocked = matches!(
            reason.as_str(),
            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT"
        );
        if blocked
            && candidate
                .content
                .as_ref()
                .is_none_or(|c| c.parts.is_empty())
        {
            return Err(LlmError::Provider(format!("finishReason: {reason}")));
        }
    }

    candidate
        .content
        .as_ref()
        .ok_or_else(|| LlmError::Provider("missing candidate content".into()))
}

#[async_trait]
impl LlmProvider for GeminiLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        let body = GenerateContentRequest {
            contents: messages_to_contents(messages),
            system_instruction: build_system_instruction(messages),
            tools: None,
            tool_config: None,
            generation_config: self.generation_config(),
        };
        let parsed = self.generate_content(body).await?;
        let content = validate_response(&parsed)?;
        let turn_index = model_turn_index(messages);
        let step = parse_candidate_parts(&content.parts, turn_index)?;
        Ok(step.assistant)
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let declarations: Vec<FunctionDeclaration> = tools
            .iter()
            .map(|t| FunctionDeclaration {
                name: t.name.as_str().to_string(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect();

        let gemini_tools = if declarations.is_empty() {
            None
        } else {
            Some(vec![GeminiTool {
                function_declarations: declarations,
            }])
        };

        let tool_config = gemini_tools.as_ref().map(|_| ToolConfig {
            function_calling_config: FunctionCallingConfig {
                mode: "AUTO".to_string(),
            },
        });

        let body = GenerateContentRequest {
            contents: messages_to_contents(messages),
            system_instruction: build_system_instruction(messages),
            tools: gemini_tools,
            tool_config,
            generation_config: self.generation_config(),
        };

        let parsed = self.generate_content(body).await?;
        let content = validate_response(&parsed)?;
        let turn_index = model_turn_index(messages);
        parse_candidate_parts(&content.parts, turn_index)
    }
}

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<Content>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<ToolConfig>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
}

#[derive(Serialize)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part {
    Text {
        text: String,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponse,
    },
    Raw(Value),
}

#[derive(Serialize)]
struct FunctionResponse {
    id: String,
    name: String,
    response: Value,
}

#[derive(Serialize)]
struct SystemInstruction {
    parts: Vec<TextPart>,
}

#[derive(Serialize)]
struct TextPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Serialize)]
struct FunctionDeclaration {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize)]
struct ToolConfig {
    #[serde(rename = "functionCallingConfig")]
    function_calling_config: FunctionCallingConfig,
}

#[derive(Serialize)]
struct FunctionCallingConfig {
    mode: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(rename = "promptFeedback")]
    prompt_feedback: Option<PromptFeedback>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct CandidateContent {
    #[serde(default)]
    parts: Vec<Value>,
}

#[derive(Deserialize)]
struct PromptFeedback {
    #[serde(rename = "blockReason")]
    block_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::READ_FILE;

    #[test]
    fn groups_consecutive_tool_messages() {
        let messages = vec![
            ChatMessage::user("go"),
            ChatMessage::tool("call_0_0", "result a"),
            ChatMessage::tool("call_0_1", "result b"),
        ];
        let contents = messages_to_contents(&messages);
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0].role, "user");
        assert_eq!(contents[1].role, "user");
        assert_eq!(contents[1].parts.len(), 2);
    }

    #[test]
    fn merges_system_into_instruction() {
        let messages = vec![
            ChatMessage {
                role: MessageRole::System,
                content: "a".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage {
                role: MessageRole::System,
                content: "b".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage::user("hi"),
        ];
        let sys = build_system_instruction(&messages).expect("system");
        assert_eq!(sys.parts[0].text, "a\n\nb");
        let contents = messages_to_contents(&messages);
        assert_eq!(contents.len(), 1);
    }

    #[test]
    fn synthetic_id_when_missing() {
        let parts = vec![json!({
            "functionCall": { "name": "read_file", "args": { "path": "x" } }
        })];
        let step = parse_candidate_parts(&parts, 2).expect("parse");
        assert_eq!(step.tool_calls[0].id, "call_2_0");
    }

    #[test]
    fn restores_thought_signature_on_part() {
        let tc = ToolCall {
            id: "c1".into(),
            name: READ_FILE.to_string(),
            arguments: json!({}),
            provider_extras: Some(json!({
                "gemini_part_index": 0,
                "thoughtSignature": "sig-abc"
            })),
        };
        let part = tool_call_to_part(&tc);
        let raw = match part {
            Part::Raw(v) => v,
            _ => panic!("expected raw part"),
        };
        assert_eq!(raw.get("thoughtSignature"), Some(&json!("sig-abc")));
    }
}
