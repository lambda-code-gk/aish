//! Gemini 3 Flashプロバイダの実装

use crate::error::{Error, env_error, http_error, json_error};
use crate::llm::provider::{LlmProvider, Message};
use serde_json::{json, Value};
use std::env;
use std::io::{BufRead, BufReader};

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
    /// * `Err(Error)` - エラーメッセージと終了コード
    pub fn new(model: Option<String>) -> Result<Self, Error> {
        let model = model.unwrap_or_else(|| "gemini-3-flash-preview".to_string());
        let api_key = env::var("GEMINI_API_KEY")
            .map_err(|_| env_error("GEMINI_API_KEY environment variable is not set"))?;
        
        Ok(Self { model, api_key })
    }
}

impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
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
            .map_err(|e| http_error(&format!("HTTP request failed: {}", e)))?;
        
        let status = response.status();
        let response_text = response.text()
            .map_err(|e| http_error(&format!("Failed to read response: {}", e)))?;
        
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
            return Err(http_error(&format!("Gemini API error: {}", error_msg)));
        }
        
        Ok(response_text)
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| json_error(&format!("Failed to parse response JSON: {}", e)))?;
        
        // エラーチェック
        if let Some(error) = v.get("error") {
            let error_msg = error["message"]
                .as_str()
                .unwrap_or("Unknown error");
            return Err(http_error(&format!("Gemini API error: {}", error_msg)));
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

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| json_error(&format!("Failed to parse response JSON: {}", e)))?;
        
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
    ) -> Result<Value, Error> {
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

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?key={}",
            self.model, self.api_key
        );
        
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(request_json.to_string())
            .send()
            .map_err(|e| http_error(&format!("HTTP request failed: {}", e)))?;
        
        let status = response.status();
        if !status.is_success() {
            let response_text = response.text()
                .map_err(|e| http_error(&format!("Failed to read response: {}", e)))?;
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err(http_error(&format!("Gemini API error: {}", error_msg)));
        }
        
        // Gemini APIはJSON配列形式でストリーミングレスポンスを返す
        // 形式: [ {JSON1} , {JSON2} , ... ]
        // ブレースカウントで完全なJSONオブジェクトを検出
        let reader = BufReader::new(response);
        let mut json_buffer = String::new();
        let mut brace_count = 0;
        let mut in_object = false;
        
        for line_result in reader.lines() {
            let line = line_result.map_err(|e| http_error(&format!("Failed to read stream line: {}", e)))?;
            
            for c in line.chars() {
                match c {
                    '{' => {
                        if !in_object {
                            in_object = true;
                            json_buffer.clear();
                        }
                        brace_count += 1;
                        json_buffer.push(c);
                    }
                    '}' => {
                        if in_object {
                            brace_count -= 1;
                            json_buffer.push(c);
                            
                            if brace_count == 0 {
                                // 完全なJSONオブジェクトを取得
                                Self::handle_json_chunk(&json_buffer, &callback)?;
                                json_buffer.clear();
                                in_object = false;
                            }
                        }
                    }
                    _ => {
                        if in_object {
                            json_buffer.push(c);
                        }
                    }
                }
            }
            
            // 行の終わりに改行を追加（JSONの整形のため）
            if in_object {
                json_buffer.push('\n');
            }
        }
        
        Ok(())
    }
}

impl GeminiProvider {
    /// JSONチャンクを処理してテキストを抽出
    fn handle_json_chunk(
        json_str: &str,
        callback: &Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        // JSONとしてパース
        let v: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return Ok(()), // パース失敗は無視（不完全なJSONの可能性）
        };
        
        // テキストを抽出
        if let Some(parts) = v["candidates"][0]["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    if !text.is_empty() {
                        callback(text)?;
                    }
                }
            }
        }
        
        Ok(())
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

