//! Echoプロバイダの実装
//!
//! このプロバイダは実際にLLM APIを呼び出さず、クエリを表示するだけです。
//! デバッグやテスト用に使用します。

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

/// Echoプロバイダ
pub struct EchoProvider;

impl EchoProvider {
    /// 新しいEchoプロバイダを作成
    pub fn new() -> Self {
        Self
    }
}

impl LlmProvider for EchoProvider {
    fn name(&self) -> &str {
        "echo"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        // クエリを表示
        println!("[Echo Provider] Request JSON:");
        println!("{}", request_json);
        
        // ダミーのレスポンスを返す（実際のAPI呼び出しは行わない）
        Ok(r#"{"echo": "This is a dummy response from echo provider"}"#.to_string())
    }

    fn parse_response_text(&self, _response_json: &str) -> Result<Option<String>, Error> {
        // Echoプロバイダは常に固定のメッセージを返す
        Ok(Some("[Echo Provider] Query received (no actual LLM call made)".to_string()))
    }

    fn check_tool_calls(&self, _response_json: &str) -> Result<bool, Error> {
        // ストリーム側でツール呼び出しをシミュレートするため、ここでは未使用
        Ok(false)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        _tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        // クエリ情報を表示（システム指示は stream_events で1回だけ表示）
        println!("[Echo Provider] Query: {}", query);
        if !history.is_empty() {
            println!("[Echo Provider] History: {} messages", history.len());
        }
        
        // シンプルなペイロードを生成
        let mut payload = json!({
            "query": query,
        });
        
        if let Some(system) = system_instruction {
            payload["system_instruction"] = json!(system);
        }
        
        if !history.is_empty() {
            let history_json: Vec<Value> = history.iter()
                .map(|msg| json!({
                    "role": msg.role,
                    "content": msg.content
                }))
                .collect();
            payload["history"] = json!(history_json);
        }
        
        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        _request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        let text = "[Echo Provider] This is a simulated streaming response from the echo provider. It displays text chunk by chunk to demonstrate the streaming capability.";
        
        for word in text.split_whitespace() {
            callback(word)?;
            callback(" ")?;
            io::stdout().flush().ok();
            thread::sleep(Duration::from_millis(50));
        }
        
        Ok(())
    }

    /// ストリームを LlmEvent に正規化。ツール定義がある場合、「call:ツール名」や「call:ツール名 {...}」でツール呼び出しをシミュレートする。
    fn stream_events(
        &self,
        request_json: &str,
        tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let payload: Value = match serde_json::from_str(request_json) {
            Ok(p) => p,
            Err(_) => {
                // パース失敗時は従来どおりテキストのみ返す
                return self.stream_events_text_only(callback);
            }
        };
        let query = payload["query"].as_str().unwrap_or("").trim();
        let history = payload["history"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);

        // ストリーム入口でもシステムプロンプトを表示
        if let Some(s) = payload.get("system_instruction").and_then(|v| v.as_str()) {
            println!("[Echo Provider] System instruction: {}", s);
        }

        // 直近がツール結果なら、テキスト応答を返して終了
        let last_is_tool = history
            .last()
            .and_then(|m| m.get("role").and_then(|r| r.as_str()))
            .map(|r| r == "tool")
            .unwrap_or(false);
        if last_is_tool {
            let result_preview = history
                .last()
                .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                .unwrap_or("{}");
            let msg = format!(
                "[Echo Provider] Tool result was received. Proceeding with: {}",
                if result_preview.len() > 60 {
                    format!("{}...", &result_preview[..60])
                } else {
                    result_preview.to_string()
                }
            );
            for word in msg.split_whitespace() {
                callback(LlmEvent::TextDelta(word.to_string()))?;
                callback(LlmEvent::TextDelta(" ".to_string()))?;
                io::stdout().flush().ok();
                thread::sleep(Duration::from_millis(30));
            }
            callback(LlmEvent::Completed {
                finish: FinishReason::Stop,
            })?;
            return Ok(());
        }

        // ツール一覧から名前を取得
        let tool_names: Vec<&str> = tools
            .map(|t| t.iter().map(|d| d.name.as_str()).collect())
            .unwrap_or_default();

        // 「call:名前」または「call: 名前」で始まる場合はツール呼び出しをシミュレート
        for name in &tool_names {
            let prefix = format!("call:{}", name);
            let prefix_sp = format!("call: {}", name);
            let is_call = query.eq_ignore_ascii_case(&prefix)
                || query.starts_with(&prefix_sp)
                || (query.len() >= prefix.len()
                    && query.get(..prefix.len()).map(|s| s.eq_ignore_ascii_case(&prefix)) == Some(true));
            if is_call {
                let args_str = query
                    .strip_prefix(&prefix)
                    .or_else(|| query.strip_prefix(&prefix_sp))
                    .or_else(|| {
                        if query.len() >= prefix.len()
                            && query.get(..prefix.len()).map(|s| s.eq_ignore_ascii_case(&prefix)) == Some(true)
                        {
                            Some(&query[prefix.len()..])
                        } else {
                            None
                        }
                    })
                    .unwrap_or("")
                    .trim();
                let args: Value = if args_str.is_empty() {
                    json!({})
                } else if let Ok(v) = serde_json::from_str(args_str) {
                    v
                } else {
                    json!({ "message": args_str, "input": args_str })
                };
                let call_id = format!("echo_call_{}", name);
                callback(LlmEvent::ToolCallBegin {
                    call_id: call_id.clone(),
                    name: (*name).to_string(),
                    thought_signature: None, // Echo provider doesn't use thought signatures
                })?;
                let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                callback(LlmEvent::ToolCallArgsDelta {
                    call_id: call_id.clone(),
                    json_fragment: args_json,
                })?;
                callback(LlmEvent::ToolCallEnd { call_id })?;
                callback(LlmEvent::Completed {
                    finish: FinishReason::ToolCalls,
                })?;
                return Ok(());
            }
        }

        // 通常のテキスト応答
        self.stream_events_text_only(callback)
    }
}

impl EchoProvider {
    fn stream_events_text_only(
        &self,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let text = "[Echo Provider] This is a simulated streaming response from the echo provider. It displays text chunk by chunk to demonstrate the streaming capability.";
        for word in text.split_whitespace() {
            callback(LlmEvent::TextDelta(word.to_string()))?;
            callback(LlmEvent::TextDelta(" ".to_string()))?;
            io::stdout().flush().ok();
            thread::sleep(Duration::from_millis(50));
        }
        callback(LlmEvent::Completed {
            finish: FinishReason::Stop,
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo_provider_name() {
        let provider = EchoProvider::new();
        assert_eq!(provider.name(), "echo");
    }

    #[test]
    fn test_echo_provider_make_request_payload() {
        let provider = EchoProvider::new();
        let payload = provider.make_request_payload("Hello", None, &[], None).unwrap();
        assert_eq!(payload["query"], "Hello");
    }

    #[test]
    fn test_echo_provider_make_request_payload_with_system() {
        let provider = EchoProvider::new();
        let payload = provider.make_request_payload("Hello", Some("You are helpful"), &[], None).unwrap();
        assert_eq!(payload["query"], "Hello");
        assert_eq!(payload["system_instruction"], "You are helpful");
    }

    #[test]
    fn test_echo_provider_make_request_payload_with_history() {
        let provider = EchoProvider::new();
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        let payload = provider.make_request_payload("How are you?", None, &history, None).unwrap();
        assert_eq!(payload["query"], "How are you?");
        assert!(payload["history"].is_array());
        assert_eq!(payload["history"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_echo_provider_parse_response_text() {
        let provider = EchoProvider::new();
        let result = provider.parse_response_text("{}").unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("Echo Provider"));
    }

    #[test]
    fn test_echo_provider_check_tool_calls() {
        let provider = EchoProvider::new();
        let result = provider.check_tool_calls("{}").unwrap();
        assert_eq!(result, false);
    }
}

