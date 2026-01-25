//! LLMドライバーの実装
//!
//! プロバイダに依存しない共通処理を提供します。

use crate::error::Error;
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
    /// * `Err(Error)` - エラーメッセージと終了コード
    pub fn query(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<String, Error> {
        use crate::error::json_error;
        
        // リクエストペイロードを生成
        let payload = self.provider.make_request_payload(query, system_instruction, history)?;
        
        // JSON文字列に変換
        let request_json = serde_json::to_string(&payload)
            .map_err(|e| json_error(&format!("Failed to serialize request: {}", e)))?;
        
        // HTTPリクエストを実行
        let response_json = self.provider.make_http_request(&request_json)?;
        
        // レスポンスからテキストを抽出
        let text = self.provider.parse_response_text(&response_json)?
            .ok_or_else(|| json_error("No text in response"))?;
        
        Ok(text)
    }

    /// LLMにクエリを送信してレスポンスをストリーミング表示
    /// 
    /// # Arguments
    /// * `query` - ユーザークエリ
    /// * `system_instruction` - システム指示（オプション）
    /// * `history` - 会話履歴（オプション）
    /// * `callback` - テキストチャンクを受け取るコールバック関数
    /// 
    /// # Returns
    /// * `Ok(())` - 成功
    /// * `Err(Error)` - エラーメッセージと終了コード
    pub fn query_streaming(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        use crate::error::json_error;
        
        // リクエストペイロードを生成
        let payload = self.provider.make_request_payload(query, system_instruction, history)?;
        
        // JSON文字列に変換
        let request_json = serde_json::to_string(&payload)
            .map_err(|e| json_error(&format!("Failed to serialize request: {}", e)))?;
        
        // ストリーミングHTTPリクエストを実行
        self.provider.make_http_streaming_request(&request_json, callback)
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

        fn make_http_request(&self, _request_json: &str) -> Result<String, Error> {
            Ok(r#"{"candidates":[{"content":{"parts":[{"text":"Hello, world!"}]}}]}"#.to_string())
        }

        fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
            use crate::error::json_error;
            let v: Value = serde_json::from_str(response_json)
                .map_err(|e| json_error(&format!("Failed to parse JSON: {}", e)))?;
            let text = v["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .map(|s| s.to_string());
            Ok(text)
        }

        fn check_tool_calls(&self, _response_json: &str) -> Result<bool, Error> {
            Ok(false)
        }

        fn make_request_payload(
            &self,
            _query: &str,
            _system_instruction: Option<&str>,
            _history: &[Message],
        ) -> Result<Value, Error> {
            Ok(serde_json::json!({
                "contents": [{
                    "role": "user",
                    "parts": [{"text": "test"}]
                }]
            }))
        }

        fn make_http_streaming_request(
            &self,
            _request_json: &str,
            callback: Box<dyn Fn(&str) -> Result<(), Error>>,
        ) -> Result<(), Error> {
            callback("Hello, ")?;
            callback("world!")?;
            Ok(())
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

    #[test]
    fn test_llm_driver_query_streaming() {
        let provider = MockProvider;
        let driver = LlmDriver::new(provider);
        let response = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let response_clone = response.clone();
        let result = driver.query_streaming(
            "test",
            None,
            &[],
            Box::new(move |chunk| {
                response_clone.lock().unwrap().push_str(chunk);
                Ok(())
            }),
        );
        assert!(result.is_ok());
        assert_eq!(*response.lock().unwrap(), "Hello, world!");
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

        fn make_http_request(&self, _request_json: &str) -> Result<String, Error> {
            use crate::error::http_error;
            match self.error_type {
                ErrorType::HttpError => Err(http_error("HTTP request failed")),
                _ => Ok(r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}]}}]}"#.to_string()),
            }
        }

        fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
            use crate::error::json_error;
            match self.error_type {
                ErrorType::ParseError => Err(json_error("Failed to parse response")),
                ErrorType::NoText => Ok(None),
                _ => {
                    let v: Value = serde_json::from_str(response_json)
                        .map_err(|e| json_error(&format!("Failed to parse JSON: {}", e)))?;
                    let text = v["candidates"][0]["content"]["parts"][0]["text"]
                        .as_str()
                        .map(|s| s.to_string());
                    Ok(text)
                }
            }
        }

        fn check_tool_calls(&self, _response_json: &str) -> Result<bool, Error> {
            Ok(false)
        }

        fn make_request_payload(
            &self,
            _query: &str,
            _system_instruction: Option<&str>,
            _history: &[Message],
        ) -> Result<Value, Error> {
            use crate::error::json_error;
            match self.error_type {
                ErrorType::PayloadError => Err(json_error("Failed to create payload")),
                _ => Ok(serde_json::json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{"text": "test"}]
                    }]
                })),
            }
        }

        fn make_http_streaming_request(
            &self,
            _request_json: &str,
            _callback: Box<dyn Fn(&str) -> Result<(), Error>>,
        ) -> Result<(), Error> {
            use crate::error::http_error;
            match self.error_type {
                ErrorType::HttpError => Err(http_error("HTTP request failed")),
                _ => Ok(()),
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

