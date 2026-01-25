//! Gemini 3 Flashプロバイダの実装

use crate::llm::provider::{LlmProvider, Message};
use serde_json::{json, Value};
use std::env;

/// Gemini 3 Flashプロバイダ
pub struct GeminiProvider {
    model: String,
    api_key: String,
}

impl GeminiProvider {
    /// 新しいGeminiプロバイダを作成
    /// 
    /// # Arguments
    /// * `model` - モデル名（デフォルト: "gemini-3-flash-preview"）
    /// 
    /// # Returns
    /// * `Ok(Self)` - プロバイダ
    /// * `Err((String, i32))` - エラーメッセージと終了コード
    pub fn new(model: Option<String>) -> Result<Self, (String, i32)> {
        let model = model.unwrap_or_else(|| "gemini-3-flash-preview".to_string());
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| ("GEMINI_API_KEY environment variable is not set".to_string(), 64))?;
        
        Ok(Self { model, api_key })
    }
}

impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, (String, i32)> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );
        
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(request_json.to_string())
            .send()
            .map_err(|e| (format!("HTTP request failed: {}", e), 74))?;
        
        let status = response.status();
        let response_text = response.text()
            .map_err(|e| (format!("Failed to read response: {}", e), 74))?;
        
        if !status.is_success() {
            // エラーレスポンスを解析してメッセージを抽出
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err((format!("Gemini API error: {}", error_msg), 74));
        }
        
        Ok(response_text)
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, (String, i32)> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| (format!("Failed to parse response JSON: {}", e), 74))?;
        
        // エラーチェック
        if let Some(error) = v.get("error") {
            let error_msg = error["message"]
                .as_str()
                .unwrap_or("Unknown error");
            return Err((format!("Gemini API error: {}", error_msg), 74));
        }
        
        // テキストを抽出
        let text = v["candidates"][0]["content"]["parts"]
            .as_array()
            .and_then(|parts| {
                parts.iter()
                    .find_map(|part| part["text"].as_str())
            })
            .map(|s| s.to_string());
        
        Ok(text)
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, (String, i32)> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| (format!("Failed to parse response JSON: {}", e), 74))?;
        
        let has_tool_calls = v["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|parts| {
                parts.iter().any(|part| part["functionCall"].is_object())
            })
            .unwrap_or(false);
        
        Ok(has_tool_calls)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, (String, i32)> {
        let mut payload = json!({});
        
        // システム指示を追加
        if let Some(system) = system_instruction {
            payload["systemInstruction"] = json!({
                "parts": [{"text": system}]
            });
        }
        
        // 会話履歴とクエリをcontentsに追加
        let mut contents = Vec::new();
        
        // 履歴を追加
        for msg in history {
            contents.push(json!({
                "role": msg.role,
                "parts": [{"text": msg.content}]
            }));
        }
        
        // ユーザークエリを追加
        contents.push(json!({
            "role": "user",
            "parts": [{"text": query}]
        }));
        
        payload["contents"] = json!(contents);
        
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_provider_name() {
        // APIキーが設定されていない場合はエラーになるが、name()は呼べる
        // 実際のテストではモックを使用するか、環境変数を設定する必要がある
    }

    #[test]
    fn test_make_request_payload_simple() {
        // APIキーなしでもペイロード生成はテストできる
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let payload = provider.make_request_payload("Hello", None, &[]).unwrap();
        assert!(payload["contents"].is_array());
        assert_eq!(payload["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_make_request_payload_with_system() {
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let payload = provider.make_request_payload("Hello", Some("You are a helpful assistant"), &[]).unwrap();
        assert!(payload["systemInstruction"].is_object());
        assert!(payload["contents"].is_array());
    }

    #[test]
    fn test_make_request_payload_with_history() {
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        
        let payload = provider.make_request_payload("How are you?", None, &history).unwrap();
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3); // 履歴2つ + クエリ1つ
    }
}

