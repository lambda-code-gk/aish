//! Echoプロバイダの実装
//!
//! このプロバイダは実際にLLM APIを呼び出さず、クエリを表示するだけです。
//! デバッグやテスト用に使用します。

use crate::llm::provider::{LlmProvider, Message};
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

    fn make_http_request(&self, request_json: &str) -> Result<String, (String, i32)> {
        // クエリを表示
        println!("[Echo Provider] Request JSON:");
        println!("{}", request_json);
        
        // ダミーのレスポンスを返す（実際のAPI呼び出しは行わない）
        Ok(r#"{"echo": "This is a dummy response from echo provider"}"#.to_string())
    }

    fn parse_response_text(&self, _response_json: &str) -> Result<Option<String>, (String, i32)> {
        // Echoプロバイダは常に固定のメッセージを返す
        Ok(Some("[Echo Provider] Query received (no actual LLM call made)".to_string()))
    }

    fn check_tool_calls(&self, _response_json: &str) -> Result<bool, (String, i32)> {
        // Echoプロバイダはtool callをサポートしない
        Ok(false)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, (String, i32)> {
        // クエリ情報を表示
        println!("[Echo Provider] Query: {}", query);
        if let Some(system) = system_instruction {
            println!("[Echo Provider] System instruction: {}", system);
        }
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
        callback: Box<dyn Fn(&str) -> Result<(), (String, i32)>>,
    ) -> Result<(), (String, i32)> {
        let text = "[Echo Provider] This is a simulated streaming response from the echo provider. It displays text chunk by chunk to demonstrate the streaming capability.";
        
        for word in text.split_whitespace() {
            callback(word)?;
            callback(" ")?;
            io::stdout().flush().ok();
            thread::sleep(Duration::from_millis(50));
        }
        
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
        let payload = provider.make_request_payload("Hello", None, &[]).unwrap();
        assert_eq!(payload["query"], "Hello");
    }

    #[test]
    fn test_echo_provider_make_request_payload_with_system() {
        let provider = EchoProvider::new();
        let payload = provider.make_request_payload("Hello", Some("You are helpful"), &[]).unwrap();
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
        let payload = provider.make_request_payload("How are you?", None, &history).unwrap();
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

