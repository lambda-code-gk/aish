//! GPTプロバイダの実装

use crate::llm::provider::{LlmProvider, Message};
use serde_json::{json, Value};
use std::env;
use std::io::{BufRead, BufReader};

/// GPTプロバイダ
pub struct GptProvider {
    model: String,
    api_key: String,
    temperature: f64,
}

impl GptProvider {
    /// 新しいGPTプロバイダを作成
    /// 
    /// # Arguments
    /// * `model` - モデル名（デフォルト: "gpt-4o"）
    /// * `temperature` - 温度パラメータ（デフォルト: 0.7）
    /// 
    /// # Returns
    /// * `Ok(Self)` - プロバイダ
    /// * `Err((String, i32))` - エラーメッセージと終了コード
    pub fn new(model: Option<String>, temperature: Option<f64>) -> Result<Self, (String, i32)> {
        let model = model.unwrap_or_else(|| "gpt-4o".to_string());
        let temperature = temperature.unwrap_or(0.7);
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| ("OPENAI_API_KEY environment variable is not set".to_string(), 64))?;
        
        Ok(Self {
            model,
            api_key,
            temperature,
        })
    }
}

impl LlmProvider for GptProvider {
    fn name(&self) -> &str {
        "gpt"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, (String, i32)> {
        let url = "https://api.openai.com/v1/chat/completions";
        
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
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
            return Err((format!("OpenAI API error: {}", error_msg), 74));
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
            return Err((format!("OpenAI API error: {}", error_msg), 74));
        }
        
        // テキストを抽出
        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string());
        
        Ok(text)
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, (String, i32)> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| (format!("Failed to parse response JSON: {}", e), 74))?;
        
        let has_tool_calls = v["choices"][0]["message"]["tool_calls"]
            .as_array()
            .map(|calls| !calls.is_empty())
            .unwrap_or(false);
        
        Ok(has_tool_calls)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, (String, i32)> {
        let mut messages = Vec::new();
        
        // システム指示を追加
        if let Some(system) = system_instruction {
            messages.push(json!({
                "role": "system",
                "content": system
            }));
        }
        
        // 履歴を追加
        for msg in history {
            messages.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        // ユーザークエリを追加
        messages.push(json!({
            "role": "user",
            "content": query
        }));
        
        let payload = json!({
            "model": self.model,
            "temperature": self.temperature,
            "messages": messages
        });
        
        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), (String, i32)>>,
    ) -> Result<(), (String, i32)> {
        let url = "https://api.openai.com/v1/chat/completions";
        
        // request_jsonに"stream": trueを追加
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| (format!("Failed to parse request JSON: {}", e), 74))?;
        payload["stream"] = json!(true);
        let streaming_request_json = serde_json::to_string(&payload)
            .map_err(|e| (format!("Failed to serialize request: {}", e), 74))?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .body(streaming_request_json)
            .send()
            .map_err(|e| (format!("HTTP request failed: {}", e), 74))?;
        
        let status = response.status();
        if !status.is_success() {
            let response_text = response.text()
                .map_err(|e| (format!("Failed to read response: {}", e), 74))?;
            let error_msg = if let Ok(v) = serde_json::from_str::<Value>(&response_text) {
                v["error"]["message"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("HTTP {}: {}", status, response_text))
            } else {
                format!("HTTP {}: {}", status, response_text)
            };
            return Err((format!("OpenAI API error: {}", error_msg), 74));
        }
        
        let reader = BufReader::new(response);
        for line_result in reader.lines() {
            let line = line_result.map_err(|e| (format!("Failed to read stream line: {}", e), 74))?;
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if data == "[DONE]" {
                    break;
                }
                
                let v: Value = serde_json::from_str(data)
                    .map_err(|e| (format!("Failed to parse stream JSON: {}", e), 74))?;
                
                if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                    callback(text)?;
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
    fn test_gpt_provider_name() {
        // APIキーが設定されていない場合はエラーになるが、name()は呼べる
        // 実際のテストではモックを使用するか、環境変数を設定する必要がある
    }

    #[test]
    fn test_make_request_payload_simple() {
        // APIキーなしでもペイロード生成はテストできる
        let provider = GptProvider {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let payload = provider.make_request_payload("Hello", None, &[]).unwrap();
        assert!(payload["messages"].is_array());
        assert_eq!(payload["messages"].as_array().unwrap().len(), 1);
        assert_eq!(payload["model"], "gpt-4o");
        assert_eq!(payload["temperature"], 0.7);
    }

    #[test]
    fn test_make_request_payload_with_system() {
        let provider = GptProvider {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let payload = provider.make_request_payload("Hello", Some("You are a helpful assistant"), &[]).unwrap();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2); // system + user
        assert_eq!(messages[0]["role"], "system");
    }

    #[test]
    fn test_make_request_payload_with_history() {
        let provider = GptProvider {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        
        let payload = provider.make_request_payload("How are you?", None, &history).unwrap();
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3); // 履歴2つ + クエリ1つ
    }
}

