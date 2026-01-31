//! Gemini 3 Flashプロバイダの実装

use crate::error::Error;
use crate::llm::events::{FinishReason, LlmEvent};
use crate::llm::provider::{LlmProvider, Message};
use crate::tool::ToolDef;
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
            .map_err(|_| Error::env("GEMINI_API_KEY environment variable is not set"))?;
        
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
            return Err(Error::http(format!("Gemini API error: {}", error_msg)));
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
            return Err(Error::http(format!("Gemini API error: {}", error_msg)));
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
            .map_err(|e| Error::json(format!("Failed to parse response JSON: {}", e)))?;
        
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
        tools: Option<&[ToolDef]>,
    ) -> Result<Value, Error> {
        let mut payload = json!({});
        
        // システム指示を追加
        if let Some(system) = system_instruction {
            payload["systemInstruction"] = json!({
                "parts": [{"text": system}]
            });
        }
        
        // ツール: グラウンディング（Google検索）+ 関数宣言（渡された場合）
        // 注意: 現在の Gemini 3 Flash Preview では googleSearch と functionDeclarations の併用が制限されている可能性があるため、
        // 関数宣言がある場合は googleSearch を含めないようにする。
        let mut tools_array = Vec::new();
        if let Some(defs) = tools {
            if !defs.is_empty() {
                let declarations: Vec<Value> = defs
                    .iter()
                    .map(|d| {
                        json!({
                            "name": d.name,
                            "description": d.description,
                            "parameters": d.parameters
                        })
                    })
                    .collect();
                tools_array.push(json!({ "functionDeclarations": declarations }));
            }
        }
        
        // ツールが空（関数宣言がない）場合のみ Google Search を追加
        if tools_array.is_empty() {
            tools_array.push(json!({ "googleSearch": {} }));
        }
        
        payload["tools"] = json!(tools_array);
        
        // 会話履歴とクエリをcontentsに追加
        let mut contents = Vec::new();
        
        // 履歴を追加
        // Gemini APIは "assistant" ではなく "model" というroleを使用する
        for msg in history {
            if msg.role == "tool" {
                // ツール結果: user ターンで functionResponse を返す
                // Gemini API では functionResponse に "name" フィールドが必須
                let name = msg.tool_name.as_deref().unwrap_or("unknown");

                // msg.content は JSON 文字列なので、Value に戻して response にセットする
                let response_json: Value = serde_json::from_str(&msg.content)
                    .unwrap_or_else(|_| serde_json::json!({ "result": msg.content }));

                contents.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": name,
                            "response": response_json
                        }
                    }]
                }));
                continue;
            }
            let role = if msg.role == "assistant" { "model" } else { &msg.role };
            let mut parts: Vec<Value> = Vec::new();
            if !msg.content.is_empty() {
                parts.push(json!({"text": msg.content}));
            }
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    let mut fc_obj = json!({
                        "functionCall": {
                            "name": tc.name,
                            "args": tc.args
                        }
                    });
                    // Gemini 3 では thoughtSignature を含める必要がある
                    if let Some(ref sig) = tc.thought_signature {
                        fc_obj["thoughtSignature"] = json!(sig);
                    }
                    parts.push(fc_obj);
                }
            }
            if parts.is_empty() {
                parts.push(json!({"text": ""}));
            }
            contents.push(json!({ "role": role, "parts": parts }));
        }
        
        // ユーザークエリを追加
        // query が空でない場合、または履歴が空の場合のみ追加する。
        // ツール実行直後の継続呼び出しでは query が空になり、history に functionResponse (role: user) が含まれているため、
        // 重複して空の user メッセージを送るのを避ける。
        if !query.is_empty() || contents.is_empty() {
            contents.push(json!({
                "role": "user",
                "parts": [{"text": query}]
            }));
        }
        
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
            return Err(Error::http(format!("Gemini API error: {}", error_msg)));
        }
        
        // Gemini APIはJSON配列形式でストリーミングレスポンスを返す
        // 形式: [ {JSON1} , {JSON2} , ... ]
        // ブレースカウントで完全なJSONオブジェクトを検出
        let reader = BufReader::new(response);
        let mut json_buffer = String::new();
        let mut brace_count = 0;
        let mut in_object = false;
        
        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
            
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

    /// ストリームを LlmEvent に正規化（テキスト + functionCall 対応）
    fn stream_events(
        &self,
        request_json: &str,
        _tools: Option<&[ToolDef]>,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
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
            return Err(Error::http(format!("Gemini API error: {}", error_msg)));
        }
        let reader = BufReader::new(response);
        let mut json_buffer = String::new();
        let mut brace_count = 0;
        let mut in_object = false;
        let mut had_tool_calls = false;
        for line_result in reader.lines() {
            let line = line_result
                .map_err(|e| Error::http(format!("Failed to read stream line: {}", e)))?;
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
                                let h = Self::handle_json_chunk_events(&json_buffer, callback)?;
                                if h {
                                    had_tool_calls = true;
                                }
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
            if in_object {
                json_buffer.push('\n');
            }
        }
        let finish = if had_tool_calls {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };
        callback(LlmEvent::Completed { finish })?;
        Ok(())
    }
}

impl GeminiProvider {
    /// JSONチャンクを LlmEvent に変換（text / functionCall 対応）
    fn handle_json_chunk_events(
        json_str: &str,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        let v: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return Ok(false),
        };
        let mut had_tool_calls = false;
        if let Some(parts) = v["candidates"][0]["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    if !text.is_empty() {
                        callback(LlmEvent::TextDelta(text.to_string()))?;
                    }
                }
                if let Some(fc) = part["functionCall"].as_object() {
                    had_tool_calls = true;
                    let name = fc["name"].as_str().unwrap_or("").to_string();
                    let call_id = fc.get("id").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| format!("call_{}", name));
                    let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                    let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                    // Gemini 3 では thoughtSignature が最初の functionCall part に含まれる
                    let thought_signature = part["thoughtSignature"].as_str().map(String::from);
                    callback(LlmEvent::ToolCallBegin {
                        call_id: call_id.clone(),
                        name: name.clone(),
                        thought_signature,
                    })?;
                    if !args_str.is_empty() && args_str != "{}" {
                        callback(LlmEvent::ToolCallArgsDelta {
                            call_id: call_id.clone(),
                            json_fragment: args_str,
                        })?;
                    }
                    callback(LlmEvent::ToolCallEnd { call_id })?;
                }
            }
        }
        Ok(had_tool_calls)
    }

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
        
        let payload = provider.make_request_payload("Hello", None, &[], None).unwrap();
        assert!(payload["contents"].is_array());
        assert_eq!(payload["contents"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_make_request_payload_with_system() {
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let payload = provider.make_request_payload("Hello", Some("You are a helpful assistant"), &[], None).unwrap();
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
        
        let payload = provider.make_request_payload("How are you?", None, &history, None).unwrap();
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3); // 履歴2つ + クエリ1つ
    }

    #[test]
    fn test_make_request_payload_converts_assistant_to_model() {
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let history = vec![
            Message::user("Hi"),
            Message::assistant("Hello!"),
            Message::user("How are you?"),
            Message::assistant("I'm doing well!"),
        ];
        
        let payload = provider.make_request_payload("What's your name?", None, &history, None).unwrap();
        let contents = payload["contents"].as_array().unwrap();
        
        // Gemini APIでは "assistant" が "model" に変換される
        assert_eq!(contents[0]["role"].as_str().unwrap(), "user");
        assert_eq!(contents[1]["role"].as_str().unwrap(), "model");  // assistant -> model
        assert_eq!(contents[2]["role"].as_str().unwrap(), "user");
        assert_eq!(contents[3]["role"].as_str().unwrap(), "model");  // assistant -> model
        assert_eq!(contents[4]["role"].as_str().unwrap(), "user");   // クエリ
    }

    #[test]
    fn test_make_request_payload_with_grounding() {
        let provider = GeminiProvider {
            model: "gemini-3-flash-preview".to_string(),
            api_key: "test-key".to_string(),
        };
        
        let payload = provider.make_request_payload("Hello", None, &[], None).unwrap();
        // グラウンディングが有効になっていることを確認
        assert!(payload["tools"].is_array());
        let tools = payload["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tools[0]["googleSearch"].is_object());
    }
}

