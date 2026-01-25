//! LLMドライバーの実装
//!
//! プロバイダに依存しない共通処理を提供します。

use crate::llm::provider::{LlmProvider, Message};

/// LLMドライバー
pub struct LlmDriver<P: LlmProvider> {
    provider: P,
}

impl<P: LlmProvider> LlmDriver<P> {
    /// 新しいドライバーを作成
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
    
    /// LLMにクエリを送信してレスポンスを取得
    /// 
    /// # Arguments
    /// * `query` - ユーザークエリ
    /// * `system_instruction` - システム指示（オプション）
    /// * `history` - 会話履歴（オプション）
    /// 
    /// # Returns
    /// * `Ok(String)` - LLMからの応答テキスト
    /// * `Err((String, i32))` - エラーメッセージと終了コード
    pub fn query(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<String, (String, i32)> {
        // リクエストペイロードを生成
        let payload = self.provider.make_request_payload(query, system_instruction, history)?;
        
        // JSON文字列に変換
        let request_json = serde_json::to_string(&payload)
            .map_err(|e| (format!("Failed to serialize request: {}", e), 74))?;
        
        // HTTPリクエストを実行
        let response_json = self.provider.make_http_request(&request_json)?;
        
        // レスポンスからテキストを抽出
        let text = self.provider.parse_response_text(&response_json)?
            .ok_or_else(|| ("No text in response".to_string(), 74))?;
        
        Ok(text)
    }
    
    /// プロバイダを取得
    pub fn provider(&self) -> &P {
        &self.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::LlmProvider;
    use serde_json::Value;

    // モックプロバイダ
    struct MockProvider;

    impl LlmProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn make_http_request(&self, _request_json: &str) -> Result<String, (String, i32)> {
            Ok(r#"{"candidates":[{"content":{"parts":[{"text":"Hello, world!"}]}}]}"#.to_string())
        }

        fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, (String, i32)> {
            let v: Value = serde_json::from_str(response_json)
                .map_err(|e| (format!("Failed to parse JSON: {}", e), 74))?;
            let text = v["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .map(|s| s.to_string());
            Ok(text)
        }

        fn check_tool_calls(&self, _response_json: &str) -> Result<bool, (String, i32)> {
            Ok(false)
        }

        fn make_request_payload(
            &self,
            _query: &str,
            _system_instruction: Option<&str>,
            _history: &[Message],
        ) -> Result<Value, (String, i32)> {
            Ok(serde_json::json!({
                "contents": [{
                    "role": "user",
                    "parts": [{"text": "test"}]
                }]
            }))
        }
    }

    #[test]
    fn test_llm_driver_new() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        assert_eq!(driver.provider().name(), "mock");
    }

    #[test]
    fn test_llm_driver_query() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", None, &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!");
    }

    #[test]
    fn test_llm_driver_query_with_system_instruction() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", Some("You are helpful"), &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!");
    }

    #[test]
    fn test_llm_driver_query_with_history() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        let result = driver.query("test", None, &history);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!");
    }

    #[test]
    fn test_llm_driver_query_with_system_and_history() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        let result = driver.query("test", Some("You are helpful"), &history);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, world!");
    }

    // エラーハンドリングのテスト用モックプロバイダ
    struct ErrorMockProvider {
        error_type: ErrorType,
    }

    enum ErrorType {
        PayloadError,
        HttpError,
        ParseError,
        NoText,
    }

    impl LlmProvider for ErrorMockProvider {
        fn name(&self) -> &str {
            "error_mock"
        }

        fn make_http_request(&self, _request_json: &str) -> Result<String, (String, i32)> {
            match self.error_type {
                ErrorType::HttpError => Err(("HTTP request failed".to_string(), 74)),
                _ => Ok(r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#.to_string()),
            }
        }

        fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, (String, i32)> {
            match self.error_type {
                ErrorType::ParseError => Err(("Failed to parse response".to_string(), 74)),
                ErrorType::NoText => Ok(None),
                _ => {
                    let v: Value = serde_json::from_str(response_json)
                        .map_err(|e| (format!("Failed to parse JSON: {}", e), 74))?;
                    let text = v["candidates"][0]["content"]["parts"][0]["text"]
                        .as_str()
                        .map(|s| s.to_string());
                    Ok(text)
                }
            }
        }

        fn check_tool_calls(&self, _response_json: &str) -> Result<bool, (String, i32)> {
            Ok(false)
        }

        fn make_request_payload(
            &self,
            _query: &str,
            _system_instruction: Option<&str>,
            _history: &[Message],
        ) -> Result<Value, (String, i32)> {
            match self.error_type {
                ErrorType::PayloadError => Err(("Failed to create payload".to_string(), 74)),
                _ => Ok(serde_json::json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{"text": "test"}]
                    }]
                })),
            }
        }
    }

    #[test]
    fn test_llm_driver_query_payload_error() {
        let provider = ErrorMockProvider {
            error_type: ErrorType::PayloadError,
        };
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", None, &[]);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Failed to create payload"));
        assert_eq!(code, 74);
    }

    #[test]
    fn test_llm_driver_query_http_error() {
        let provider = ErrorMockProvider {
            error_type: ErrorType::HttpError,
        };
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", None, &[]);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("HTTP request failed"));
        assert_eq!(code, 74);
    }

    #[test]
    fn test_llm_driver_query_parse_error() {
        let provider = ErrorMockProvider {
            error_type: ErrorType::ParseError,
        };
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", None, &[]);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("Failed to parse response"));
        assert_eq!(code, 74);
    }

    #[test]
    fn test_llm_driver_query_no_text() {
        let provider = ErrorMockProvider {
            error_type: ErrorType::NoText,
        };
        let driver = LlmDriver::new(provider);
        let result = driver.query("test", None, &[]);
        assert!(result.is_err());
        let (msg, code) = result.unwrap_err();
        assert!(msg.contains("No text in response"));
        assert_eq!(code, 74);
    }

    // Echoプロバイダを使った実際のテスト
    #[test]
    fn test_llm_driver_with_echo_provider() {
        use crate::llm::echo::EchoProvider;
        let provider = EchoProvider::new();
        let driver = LlmDriver::new(provider);
        let result = driver.query("Hello, echo!", None, &[]);
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.contains("Echo Provider"));
    }

    #[test]
    fn test_llm_driver_with_echo_provider_and_system() {
        use crate::llm::echo::EchoProvider;
        let provider = EchoProvider::new();
        let driver = LlmDriver::new(provider);
        let result = driver.query("Hello", Some("You are helpful"), &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_llm_driver_with_echo_provider_and_history() {
        use crate::llm::echo::EchoProvider;
        let provider = EchoProvider::new();
        let driver = LlmDriver::new(provider);
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        let result = driver.query("How are you?", None, &history);
        assert!(result.is_ok());
    }
}

