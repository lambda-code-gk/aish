//! GPTプロバイダの実装

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
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
    /// * `model` - モデル名（デフォルト: "gpt-5.2"）
    /// * `temperature` - 温度パラメータ（デフォルト: 0.7）
    /// 
    /// # Returns
    /// * `Ok(Self)` - プロバイダ
    /// * `Err(Error)` - エラーメッセージと終了コード
    pub fn new(model: Option<String>, temperature: Option<f64>) -> Result<Self, Error> {
        let model = model.unwrap_or_else(|| "gpt-5.2".to_string());
        let temperature = temperature.unwrap_or(0.7);
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| Error::env("OPENAI_API_KEY environment variable is not set"))?;
        
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

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        let url = "https://api.openai.com/v1/responses";
        
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .body(request_json.to_string())
            .send()
            .map_err(|e| Error::http(format!("HTTP request failed: {}", e)))?;
        
        let status = response.status();
        let response_text = response
            .text()
            .map_err(|e| Error::http(format!("Failed to read response: {}", e)))?;
        
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
            return Err(Error::http(format!("OpenAI API error: {}", error_msg)));
        }
        
        Ok(response_text)
    }

    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| Error::json(format!("Failed to parse response JSON: {}", e)))?;
        
        // エラーチェック
        if let Some(error) = v.get("error") {
            let error_msg = error["message"]
                .as_str()
                .unwrap_or("Unknown error");
            return Err(Error::http(format!("OpenAI API error: {}", error_msg)));
        }
        
        // テキストを抽出（Responses API形式）
        // 実際のAPIレスポンス形式: response.output[0].content[0].text
        let text = v["response"]["output"]
            .as_array()
            .and_then(|outputs| outputs.get(0))
            .and_then(|output| output["content"].as_array())
            .and_then(|contents| contents.get(0))
            .and_then(|content| content["text"].as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                // フォールバック: 直接output_textを試す
                v["output_text"]
                    .as_str()
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                // フォールバック: ネストされた形式を試す
                v["response"]["output_text"]
                    .as_str()
                    .map(|s| s.to_string())
            })
            .or_else(|| {
                // フォールバック: Chat Completions形式も試す（後方互換性のため）
                v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(|s| s.to_string())
            });
        
        Ok(text)
    }

    fn check_tool_calls(&self, response_json: &str) -> Result<bool, Error> {
        let v: Value = serde_json::from_str(response_json)
            .map_err(|e| Error::json(format!("Failed to parse response JSON: {}", e)))?;
        
        // Responses API形式を試す（形式が不明なため、複数の可能性をチェック）
        let has_tool_calls = v["tool_calls"]
            .as_array()
            .map(|calls| !calls.is_empty())
            .unwrap_or_else(|| {
                // フォールバック: Chat Completions形式も試す（後方互換性のため）
                v["choices"][0]["message"]["tool_calls"]
                    .as_array()
                    .map(|calls| !calls.is_empty())
                    .unwrap_or(false)
            });
        
        Ok(has_tool_calls)
    }

    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, Error> {
        // Responses API形式: inputにメッセージ配列を設定
        let mut input = Vec::new();
        
        // Responses API: input[] に tool_calls は入れない（未知パラメータで弾かれる）。
        // ツール結果は input item の type "tool_result" で送る（role "tool" ではない）。
        for msg in history {
            if msg.role == "tool" {
                if let Some(ref call_id) = msg.tool_call_id {
                    input.push(json!({
                        "type": "tool_result",
                        "tool_call_id": call_id,
                        "output": msg.content
                    }));
                }
                continue;
            }
            input.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        // ユーザークエリを追加
        input.push(json!({
            "role": "user",
            "content": query
        }));
        
        let mut payload = json!({
            "model": self.model,
            "temperature": self.temperature,
            "input": input,
            "store": false  // クライアント側で履歴管理
        });
        
        // システム指示はinstructionsパラメータで指定
        if let Some(system) = system_instruction {
            payload["instructions"] = json!(system);
        }
        
        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        let url = "https://api.openai.com/v1/responses";
        
        // request_jsonに"stream": trueを追加
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let streaming_request_json = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .body(streaming_request_json)
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
            return Err(Error::http(format!("OpenAI API error: {}", error_msg)));
        }
        
        let reader = BufReader::new(response);
        let mut found_any_text = false;
        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if data == "[DONE]" {
                    break;
                }
                
                let v: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_e) => {
                        // パースエラーは無視（不完全なJSONの可能性）
                        continue;
                    }
                };
                
                // Responses API形式: typeが"response.output_text.delta"の場合、deltaフィールドにテキストがある
                let mut text_found = false;
                if let Some(type_str) = v["type"].as_str() {
                    if type_str == "response.output_text.delta" {
                        if let Some(text) = v["delta"].as_str() {
                            callback(text)?;
                            text_found = true;
                            found_any_text = true;
                        }
                    }
                }
                
                // フォールバック: 他の形式も試す（text_foundがfalseの場合のみ）
                if !text_found {
                    if let Some(text) = v["delta"]["content"].as_str() {
                        callback(text)?;
                        found_any_text = true;
                    } else if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                        // フォールバック: Chat Completions形式も試す（後方互換性のため）
                        callback(text)?;
                        found_any_text = true;
                    }
                }
            }
        }
        
        // テキストが全く見つからない場合、エラーを返す
        if !found_any_text {
            return Err(Error::json("No text found in streaming response"));
        }
        
        Ok(())
    }

    /// ストリームを LlmEvent に正規化し、チャンク受信ごとに即コールバック（ストリーミング表示）
    fn stream_events(
        &self,
        request_json: &str,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let url = "https://api.openai.com/v1/responses";
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let streaming_request_json = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .body(streaming_request_json)
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
            return Err(Error::http(format!("OpenAI API error: {}", error_msg)));
        }

        let reader = BufReader::new(response);
        let mut found_any_text = false;
        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if data == "[DONE]" {
                    break;
                }

                let v: Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_e) => continue,
                };

                let mut text_found = false;
                if let Some(type_str) = v["type"].as_str() {
                    if type_str == "response.output_text.delta" {
                        if let Some(text) = v["delta"].as_str() {
                            callback(LlmEvent::TextDelta(text.to_string()))?;
                            text_found = true;
                            found_any_text = true;
                        }
                    }
                }
                if !text_found {
                    if let Some(text) = v["delta"]["content"].as_str() {
                        callback(LlmEvent::TextDelta(text.to_string()))?;
                        found_any_text = true;
                    } else if let Some(text) = v["choices"][0]["delta"]["content"].as_str() {
                        callback(LlmEvent::TextDelta(text.to_string()))?;
                        found_any_text = true;
                    }
                }
            }
        }

        if !found_any_text {
            return Err(Error::json("No text found in streaming response"));
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
    fn test_gpt_provider_name() {
        // APIキーが設定されていない場合はエラーになるが、name()は呼べる
        // 実際のテストではモックを使用するか、環境変数を設定する必要がある
    }

    #[test]
    fn test_make_request_payload_simple() {
        // APIキーなしでもペイロード生成はテストできる
        let provider = GptProvider {
            model: "gpt-5.2".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let payload = provider.make_request_payload("Hello", None, &[]).unwrap();
        assert!(payload["input"].is_array());
        assert_eq!(payload["input"].as_array().unwrap().len(), 1);
        assert_eq!(payload["model"], "gpt-5.2");
        assert_eq!(payload["temperature"], 0.7);
        assert_eq!(payload["store"], false);
    }

    #[test]
    fn test_make_request_payload_with_system() {
        let provider = GptProvider {
            model: "gpt-5.2".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let payload = provider.make_request_payload("Hello", Some("You are a helpful assistant"), &[]).unwrap();
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 1); // user only (system is in instructions)
        assert_eq!(payload["instructions"], "You are a helpful assistant");
    }

    #[test]
    fn test_make_request_payload_with_history() {
        let provider = GptProvider {
            model: "gpt-5.2".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
        };
        
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        
        let payload = provider.make_request_payload("How are you?", None, &history).unwrap();
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 3); // 履歴2つ + クエリ1つ
    }
}

