//! GPTプロバイダの実装

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader};

const DEFAULT_GPT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_GPT_API_KEY_ENV: &str = "OPENAI_API_KEY";

/// GPTプロバイダ（Responses API）
pub struct GptProvider {
    model: String,
    api_key: String,
    temperature: f64,
    base_url: String,
}

impl GptProvider {
    /// 新しいGPTプロバイダを作成
    ///
    /// # Arguments
    /// * `model` - モデル名（None のとき "gpt-5.2"）
    /// * `temperature` - 温度（None のとき 0.7）
    /// * `base_url` - ベース URL（None のとき DEFAULT_GPT_BASE_URL）。末尾スラッシュは除いて "{base_url}/responses" で利用
    /// * `api_key_env` - API キーを読む環境変数名（None のとき OPENAI_API_KEY）
    pub fn new(
        model: Option<String>,
        temperature: Option<f64>,
        base_url: Option<String>,
        api_key_env: Option<String>,
    ) -> Result<Self, Error> {
        let model = model.unwrap_or_else(|| "gpt-5.2".to_string());
        let temperature = temperature.unwrap_or(0.7);
        let base_url = base_url
            .unwrap_or_else(|| DEFAULT_GPT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let env_name = api_key_env.as_deref().unwrap_or(DEFAULT_GPT_API_KEY_ENV);
        let api_key = env::var(env_name)
            .map_err(|_| Error::env(format!("{} environment variable is not set", env_name)))?;

        Ok(Self {
            model,
            api_key,
            temperature,
            base_url,
        })
    }

    fn url(&self) -> String {
        format!("{}/responses", self.base_url)
    }
}

impl LlmProvider for GptProvider {
    fn name(&self) -> &str {
        "gpt"
    }

    fn make_http_request(&self, request_json: &str) -> Result<String, Error> {
        let client = reqwest::blocking::Client::new();
        let response = client
            .post(self.url())
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
        tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        // Responses API形式: inputにメッセージ配列を設定。
        // function_call_output を送る場合は、その前に同じ call_id の function_call が input に含まれている必要がある。
        let mut input = Vec::new();
        
        for msg in history {
            if msg.role == "tool" {
                if let Some(ref call_id) = msg.tool_call_id {
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": msg.content
                    }));
                }
                continue;
            }
            // アシスタントが tool_calls を持つ場合: まず message を追加し、続けて各 function_call を追加する
            if msg.role == "assistant" {
                // content が空でない場合のみ message アイテムを追加
                if !msg.content.trim().is_empty() {
                input.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": msg.content
                        }
                    ]
                }));
                }
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let arguments_str = serde_json::to_string(&tc.args)
                            .unwrap_or_else(|_| "{}".to_string());
                        input.push(json!({
                            "type": "function_call",
                            "call_id": tc.id,
                            "name": tc.name,
                            "arguments": arguments_str
                        }));
                    }
                }
                continue;
            }
            if msg.role == "user" {
                input.push(json!({
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": msg.content
                        }
                    ]
                }));
                continue;
            }
            input.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }
        
        // ユーザークエリを追加
        input.push(json!({
            "type": "message",
            "role": "user",
            "content": [
                {
                    "type": "input_text",
                    "text": query
                }
            ]
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
        
        // ツール定義（Responses API形式: type, name, description, parameters をトップレベルに）
        // Chat Completions の "function": { name, description, parameters } とは異なる
        if let Some(defs) = tools {
            if !defs.is_empty() {
                let tools_json: Vec<Value> = defs
                    .iter()
                    .map(|d| {
                        json!({
                            "type": "function",
                            "name": d.name,
                            "description": d.description,
                            "parameters": d.parameters
                        })
                    })
                    .collect();
                payload["tools"] = json!(tools_json);
            }
        }
        
        Ok(payload)
    }

    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error> {
        // request_jsonに"stream": trueを追加
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let streaming_request_json = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(self.url())
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
        _tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut payload: Value = serde_json::from_str(request_json)
            .map_err(|e| Error::json(format!("Failed to parse request JSON: {}", e)))?;
        payload["stream"] = json!(true);
        let streaming_request_json = serde_json::to_string(&payload)
            .map_err(|e| Error::json(format!("Failed to serialize request: {}", e)))?;

        let client = reqwest::blocking::Client::new();
        let response = client
            .post(self.url())
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
        let mut had_tool_calls = false;
        // item_id (function_call の id) -> (call_id, name)。arguments.done で ToolCallEnd を出すために保持
        let mut item_to_call: HashMap<String, (String, String)> = HashMap::new();
        let mut pending_args: HashMap<String, String> = HashMap::new();

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

                let type_str = match v["type"].as_str() {
                    Some(s) => s,
                    None => continue,
                };

                // テキストデルタ
                if type_str == "response.output_text.delta" {
                    if let Some(text) = v["delta"].as_str() {
                        callback(LlmEvent::TextDelta(text.to_string()))?;
                        found_any_text = true;
                    }
                    continue;
                }
                // done は完了通知のみ。本文は delta で既に送っているため、ここで TextDelta を出さない（二重表示防止）
                if type_str == "response.output_text.done" {
                    if v["text"].as_str().is_some() {
                        found_any_text = true;
                    }
                    continue;
                }

                // function_call: output_item.added で ToolCallBegin
                if type_str == "response.output_item.added" {
                    if let Some(item) = v["item"].as_object() {
                        if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                            let item_id = item
                                .get("id")
                                .and_then(|id| id.as_str())
                                .unwrap_or("")
                                .to_string();
                            let call_id = item
                                .get("call_id")
                                .and_then(|c| c.as_str())
                                .unwrap_or(&item_id)
                                .to_string();
                            let name = item
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();
                            item_to_call.insert(item_id.clone(), (call_id.clone(), name.clone()));
                            pending_args.insert(item_id, String::new());
                            callback(LlmEvent::ToolCallBegin {
                                call_id: call_id.clone(),
                                name: name.clone(),
                                thought_signature: None, // OpenAI doesn't use thought signatures
                            })?;
                            had_tool_calls = true;
                        }
                    }
                    continue;
                }

                // function_call 引数デルタ（蓄積して done でまとめて送る）
                if type_str == "response.function_call_arguments.delta" {
                    if let Some(item_id) = v["item_id"].as_str() {
                        if let Some(delta) = v["delta"].as_str() {
                            pending_args
                                .entry(item_id.to_string())
                                .or_default()
                                .push_str(delta);
                        }
                    }
                    continue;
                }

                // function_call 引数完了 → ToolCallArgsDelta + ToolCallEnd
                if type_str == "response.function_call_arguments.done" {
                    let item_id = v["item_id"].as_str().unwrap_or("").to_string();
                    let arguments = v["arguments"].as_str().unwrap_or("{}").to_string();
                    if let Some((call_id, _name)) = item_to_call.remove(&item_id) {
                        pending_args.remove(&item_id);
                        if !arguments.is_empty() && arguments != "{}" {
                            callback(LlmEvent::ToolCallArgsDelta {
                                call_id: call_id.clone(),
                                json_fragment: arguments,
                            })?;
                        }
                        callback(LlmEvent::ToolCallEnd { call_id })?;
                    }
                    continue;
                }

                // フォールバック: 旧形式のテキスト
                if type_str != "response.output_item.added"
                    && type_str != "response.output_item.done"
                {
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

        // output_item.added だけで arguments.done が来ないケース: 蓄積した args で ToolCallEnd まで出す
        for (item_id, args_buf) in pending_args {
            if let Some((call_id, _)) = item_to_call.remove(&item_id) {
                if !args_buf.trim().is_empty() {
                    callback(LlmEvent::ToolCallArgsDelta {
                        call_id: call_id.clone(),
                        json_fragment: args_buf,
                    })?;
                }
                callback(LlmEvent::ToolCallEnd { call_id })?;
            }
        }

        if !found_any_text && !had_tool_calls {
            return Err(Error::json("No text found in streaming response"));
        }

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
            base_url: DEFAULT_GPT_BASE_URL.to_string(),
        };

        let payload = provider.make_request_payload("Hello", None, &[], None).unwrap();
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
            base_url: DEFAULT_GPT_BASE_URL.to_string(),
        };

        let payload = provider.make_request_payload("Hello", Some("You are a helpful assistant"), &[], None).unwrap();
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
            base_url: DEFAULT_GPT_BASE_URL.to_string(),
        };

        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
        ];
        
        let payload = provider.make_request_payload("How are you?", None, &history, None).unwrap();
        let input = payload["input"].as_array().unwrap();
        assert_eq!(input.len(), 3); // 履歴2つ + クエリ1つ
    }

    #[test]
    fn test_make_request_payload_with_tool_calls_in_history() {
        let provider = GptProvider {
            model: "gpt-5.2".to_string(),
            api_key: "test-key".to_string(),
            temperature: 0.7,
            base_url: DEFAULT_GPT_BASE_URL.to_string(),
        };
        // ユーザー → アシスタント（echo 呼び出し）→ function_call → function_call_output → 次のクエリ
        let history = vec![
            Message::user("echoツールを使って"),
            Message::assistant_with_tool_calls(
                "", // 空の content
                vec![(
                    "call_HwXgpXDh9H9C3asidWlO9r6H".to_string(),
                    "echo".to_string(),
                    serde_json::json!({"message": "hello"}),
                    None, // thought_signature (not used by OpenAI)
                )],
            ),
            Message::tool_result("call_HwXgpXDh9H9C3asidWlO9r6H", "echo", "hello"),
        ];
        let payload = provider.make_request_payload("続けて", None, &history, None).unwrap();
        let input = payload["input"].as_array().unwrap();
        // user, function_call (assistant message は空なのでスキップ), function_call_output, 今回の user
        assert_eq!(input.len(), 4);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_HwXgpXDh9H9C3asidWlO9r6H");
        assert_eq!(input[1]["name"], "echo");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["call_id"], "call_HwXgpXDh9H9C3asidWlO9r6H");
        assert_eq!(input[3]["type"], "message");
        assert_eq!(input[3]["role"], "user");
    }
}

