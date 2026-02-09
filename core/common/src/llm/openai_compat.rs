//! OpenAI Chat Completions 互換 (/chat/completions) プロバイダ
//!
//! base_url で任意のエンドポイントを指定可能。tool_calls とストリーミングを LlmEvent に正規化する。

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
use serde_json::{json, Value};
use std::env;
use std::io::{BufRead, BufReader};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_TEMPERATURE: f64 = 0.7;

/// OpenAI Chat Completions 互換プロバイダ
pub struct OpenAiCompatProvider {
    model: String,
    base_url: String,
    api_key_env: Option<String>,
    temperature: f64,
}

impl OpenAiCompatProvider {
    /// 新しいプロバイダを作成
    ///
    /// * `model` - モデル名（None のとき "gpt-4o-mini"）
    /// * `base_url` - ベース URL（None のとき DEFAULT_BASE_URL）
    /// * `api_key_env` - API キーを読む環境変数名（None のとき Authorization を付けない）
    /// * `temperature` - 温度（None のとき DEFAULT_TEMPERATURE）
    pub fn new(
        model: Option<String>,
        base_url: Option<String>,
        api_key_env: Option<String>,
        temperature: Option<f32>,
    ) -> Result<Self, Error> {
        let model = model.unwrap_or_else(|| "gpt-4o-mini".to_string());
        let base_url = base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let temperature = temperature.map(f64::from).unwrap_or(DEFAULT_TEMPERATURE);
        Ok(Self {
            model,
            base_url,
            api_key_env,
            temperature,
        })
    }

    fn url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn auth_header(&self) -> Option<String> {
        self.api_key_env.as_ref().and_then(|name| {
            env::var(name).ok().map(|key| format!("Bearer {}", key))
        })
    }
}

impl LlmProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        "openai_compat"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        let mut builder = reqwest::blocking::Client::new()
            .post(self.url())
            .header("Content-Type", "application/json")
            .body(request_json.to_string());

        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }

        let response = builder
            .send()
            .map_err(|e| Error::http(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let response_text = response
            .text()
            .map_err(|e| Error::http(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err(Error::http(format!("Chat completions error: {}", error_msg)));
        }

        Ok(response_text)
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| Error::json(format!("Failed to parse response JSON: {}", e)))?;

        if let Some(err) = v.get("error") {
            let msg = err["message"].as_str().unwrap_or("Unknown error");
            return Err(Error::http(format!("API error: {}", msg)));
        }

        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string());
        Ok(text)
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| Error::json(format!("Failed to parse response JSON: {}", e)))?;

        let has = v["choices"][0]["message"]["tool_calls"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        Ok(has)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        let mut messages: Vec<Value> = Vec::new();

        if let Some(s) = system_instruction {
            messages.push(json!({ "role": "system", "content": s }));
        }

        for msg in history {
            if msg.role == "system" {
                messages.push(json!({ "role": "system", "content": msg.content }));
                continue;
            }
            if msg.role == "user" {
                messages.push(json!({ "role": "user", "content": msg.content }));
                continue;
            }
            if msg.role == "tool" {
                let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                messages.push(json!({
                    "role": "tool",
                    "content": msg.content,
                    "tool_call_id": call_id
                }));
                continue;
            }
            if msg.role == "assistant" {
                if let Some(ref tool_calls) = msg.tool_calls {
                    let openai_tool_calls: Vec<Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            let args = serde_json::to_string(&tc.args).unwrap_or_else(|_| "{}".to_string());
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": args
                                }
                            })
                        })
                        .collect();
                    messages.push(json!({
                        "role": "assistant",
                        "content": msg.content,
                        "tool_calls": openai_tool_calls
                    }));
                } else {
                    messages.push(json!({
                        "role": "assistant",
                        "content": msg.content
                    }));
                }
                continue;
            }
            messages.push(json!({ "role": msg.role, "content": msg.content }));
        }

        messages.push(json!({ "role": "user", "content": query }));

        let mut payload = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
            "stream": false
        });

        if let Some(defs) = tools {
            if !defs.is_empty() {
                let tools_json: Vec<Value> = defs
                    .iter()
                    .map(|d| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": d.name,
                                "description": d.description,
                                "parameters": d.parameters
                            }
                        })
                    })
                    .collect();
                payload["tools"] = json!(tools_json);
                payload["tool_choice"] = json!("auto");
            }
        }

        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let body = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let mut builder = reqwest::blocking::Client::new()
            .post(self.url())
            .header("Content-Type", "application/json")
            .body(body);

        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }

        let response = builder
            .send()
            .map_err(|e| Error::http(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let response_text = response
                .text()
                .map_err(|e| Error::http(format!("Failed to read response: {}", e)))?;
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err(Error::http(format!("Chat completions error: {}", error_msg)));
        }

        let reader = BufReader::new(response);
        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
            if line.starts_with("data: ") {
                let data = line["data: ".len()..].trim();
                if data == "[DONE]" {
                    break;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                        if !text.is_empty() {
                            callback(text)?;
                        }
                    }
                    // reasoning_content: DeepSeek R1 系の推論モデル対応
                    if let Some(text) = v["choices"][0]["delta"]["reasoning_content"].as_str() {
                        if !text.is_empty() {
                            callback(text)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn stream_events(
        &self,
        request_json: &str,
        _tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let body = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let mut builder = reqwest::blocking::Client::new()
            .post(self.url())
            .header("Content-Type", "application/json")
            .body(body);

        if let Some(auth) = self.auth_header() {
            builder = builder.header("Authorization", auth);
        }

        let response = builder
            .send()
            .map_err(|e| Error::http(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let response_text = response
                .text()
                .map_err(|e| Error::http(format!("Failed to read response: {}", e)))?;
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err(Error::http(format!("Chat completions error: {}", error_msg)));
        }

        let reader = BufReader::new(response);
        let mut _found_any = false;
        let mut had_tool_calls = false;
        // index -> (call_id, name). Streaming sends tool_calls by index.
        let mut index_to_call: std::collections::HashMap<usize, (String, String)> =
            std::collections::HashMap::new();
        let mut index_to_args: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();

        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
            if !line.starts_with("data: ") {
                continue;
            }
            let data = line["data: ".len()..].trim();
            if data == "[DONE]" {
                break;
            }

            let v: Value = match serde_json::from_str(data) {
                Ok(x) => x,
                Err(_) => continue,
            };

            let delta = match v["choices"].get(0).and_then(|c| c.get("delta")) {
                Some(d) => d,
                None => continue,
            };

            // content: 文字列のほか、OpenAI 互換の content parts 配列にも対応
            if let Some(s) = delta["content"].as_str() {
                if !s.is_empty() {
                    callback(LlmEvent::TextDelta(s.to_string()))?;
                    _found_any = true;
                }
            } else if let Some(parts) = delta["content"].as_array() {
                for part in parts {
                    if let Some(text) = part["text"].as_str() {
                        if !text.is_empty() {
                            callback(LlmEvent::TextDelta(text.to_string()))?;
                            _found_any = true;
                        }
                    }
                }
            }

            // reasoning_content: DeepSeek R1 系の推論モデルが使用するフィールド。
            // content が空のとき、reasoning_content にテキストが入る場合がある。
            if let Some(s) = delta["reasoning_content"].as_str() {
                if !s.is_empty() {
                    callback(LlmEvent::TextDelta(s.to_string()))?;
                    _found_any = true;
                }
            }

            if let Some(tool_calls) = delta["tool_calls"].as_array() {
                for tc in tool_calls {
                    let index = tc["index"].as_u64().unwrap_or(0) as usize;
                    if let Some(id) = tc["id"].as_str() {
                        let name = tc["function"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        index_to_call.insert(index, (id.to_string(), name.clone()));
                        index_to_args.insert(index, String::new());
                        callback(LlmEvent::ToolCallBegin {
                            call_id: id.to_string(),
                            name,
                            thought_signature: None,
                        })?;
                        had_tool_calls = true;
                    }
                    if let Some(args_delta) = tc["function"]["arguments"].as_str() {
                        if !args_delta.is_empty() {
                            index_to_args
                                .entry(index)
                                .or_default()
                                .push_str(args_delta);
                        }
                    }
                }
            }
        }

        let mut indices: Vec<usize> = index_to_call.keys().copied().collect();
        indices.sort_unstable();
        for idx in indices {
            let (call_id, _) = index_to_call.get(&idx).unwrap().clone();
            if let Some(args) = index_to_args.remove(&idx) {
                if !args.trim().is_empty() {
                    callback(LlmEvent::ToolCallArgsDelta {
                        call_id: call_id.clone(),
                        json_fragment: args,
                    })?;
                }
            }
            callback(LlmEvent::ToolCallEnd { call_id })?;
        }

        // 空ストリーム（content も tool_calls もなし）はエラーにせず正常終了する。
        // 一部の openai_compat バックエンド（例: ツール実行後の続きで空の delta のみ返す場合）に対応。
        callback(LlmEvent::Completed {
            finish: if had_tool_calls {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            },
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_compat_make_request_payload_simple() {
        let p = OpenAiCompatProvider::new(
            Some("gpt-4o-mini".to_string()),
            Some("https://api.example.com/v1".to_string()),
            None,
            Some(0.5),
        )
        .unwrap();
        let payload = p.make_request_payload("Hello", None, &[], None).unwrap();
        assert_eq!(payload["model"], "gpt-4o-mini");
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["stream"], false);
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "Hello");
        assert_eq!(p.url(), "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn test_openai_compat_make_request_payload_with_system_and_history() {
        let p = OpenAiCompatProvider::new(None, None, None, None).unwrap();
        let payload = p
            .make_request_payload(
                "Hi",
                Some("You are helpful."),
                &[Message::user("A"), Message::assistant("B")],
                None,
            )
            .unwrap();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "A");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["content"], "B");
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(messages[3]["content"], "Hi");
    }

    #[test]
    fn test_openai_compat_make_request_payload_with_tool_calls_and_tools() {
        let p = OpenAiCompatProvider::new(None, None, None, None).unwrap();
        let history = vec![
            Message::assistant_with_tool_calls(
                "",
                vec![(
                    "call_1".to_string(),
                    "run_shell".to_string(),
                    json!({"cmd": "ls"}),
                    None,
                )],
            ),
            Message::tool_result("call_1", "run_shell", "done"),
        ];
        let tools = &[ToolDef {
            name: "run_shell".to_string(),
            description: "Run a shell command".to_string(),
            parameters: json!({"type": "object"}),
        }];
        let payload = p
            .make_request_payload("next", None, &history, Some(tools))
            .unwrap();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert!(messages[0]["tool_calls"].is_array());
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "run_shell");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_1");
        assert_eq!(payload["tools"][0]["function"]["name"], "run_shell");
        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn test_openai_compat_parse_response_text() {
        let p = OpenAiCompatProvider::new(None, None, None, None).unwrap();
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"Hello world"}}]}"#;
        let text = p.parse_response_text(json).unwrap();
        assert_eq!(text.as_deref(), Some("Hello world"));
    }

    #[test]
    fn test_openai_compat_parse_response_text_empty_content() {
        let p = OpenAiCompatProvider::new(None, None, None, None).unwrap();
        let json = r#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#;
        let text = p.parse_response_text(json).unwrap();
        assert_eq!(text, None);
    }

    #[test]
    fn test_openai_compat_check_tool_calls() {
        let p = OpenAiCompatProvider::new(None, None, None, None).unwrap();
        let json = r#"{"choices":[{"message":{"tool_calls":[{"id":"c1","function":{"name":"f"}}]}}]}"#;
        assert!(p.check_tool_calls(json).unwrap());
        let json_no_tools = r#"{"choices":[{"message":{"content":"Hi"}}]}"#;
        assert!(!p.check_tool_calls(json_no_tools).unwrap());
    }

    /// SSE 1行（data: {...}）をパースして choices[0].delta の形を検証
    #[test]
    fn test_openai_compat_sse_delta_content_parse() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#;
        let data = line.strip_prefix("data: ").unwrap().trim();
        let v: Value = serde_json::from_str(data).unwrap();
        let content = v["choices"][0]["delta"]["content"].as_str().unwrap();
        assert_eq!(content, "Hello");
    }

    /// SSE 1行で tool_calls の delta をパース
    #[test]
    fn test_openai_compat_sse_delta_tool_calls_parse() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"run_shell","arguments":"{\"x\":1}"}}]}}]}"#;
        let data = line.strip_prefix("data: ").unwrap().trim();
        let v: Value = serde_json::from_str(data).unwrap();
        let tool_calls = v["choices"][0]["delta"]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["index"], 0);
        assert_eq!(tool_calls[0]["id"], "call_abc");
        assert_eq!(tool_calls[0]["function"]["name"], "run_shell");
        assert_eq!(tool_calls[0]["function"]["arguments"], "{\"x\":1}");
    }

}
