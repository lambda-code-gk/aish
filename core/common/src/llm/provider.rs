//! LLMプロバイダのトレイト定義

use crate::error::Error;
use crate::llm::events::LlmEvent;
use serde_json::Value;

/// LLMプロバイダのトレイト
/// 
/// 各プロバイダ（Gemini、GPTなど）はこのトレイトを実装する必要があります。
pub trait LlmProvider {
    /// プロバイダ名を返す
    fn name(&self) -> &str;
    
    /// HTTPリクエストを実行してレスポンスを取得
    /// 
    /// # Arguments
    /// * `request_json` - リクエストJSON文字列
    /// 
    /// # Returns
    /// * `Ok(String)` - レスポンスJSON文字列
    /// * `Err(Error)` - エラーメッセージと終了コード
    fn make_http_request(&self, request_json: &str) -> Result<String, Error>;
    
    /// レスポンスからテキストを抽出
    /// 
    /// # Arguments
    /// * `response_json` - レスポンスJSON文字列
    /// 
    /// # Returns
    /// * `Ok(Option<String>)` - 抽出したテキスト（存在しない場合はNone）
    /// * `Err(Error)` - エラーメッセージと終了コード
    fn parse_response_text(&self, response_json: &str) -> Result<Option<String>, Error>;
    
    /// tool/function callの有無をチェック
    /// 
    /// # Arguments
    /// * `response_json` - レスポンスJSON文字列
    /// 
    /// # Returns
    /// * `Ok(bool)` - tool callがある場合はtrue
    /// * `Err(Error)` - エラーメッセージと終了コード
    fn check_tool_calls(&self, response_json: &str) -> Result<bool, Error>;
    
    /// リクエストペイロードを生成（通常モード）
    /// 
    /// # Arguments
    /// * `query` - ユーザークエリ
    /// * `system_instruction` - システム指示（オプション）
    /// * `history` - 会話履歴（オプション）
    /// 
    /// # Returns
    /// * `Ok(Value)` - リクエストJSON
    /// * `Err(Error)` - エラーメッセージと終了コード
    fn make_request_payload(
        &self,
        query: &str,
        system_instruction: Option<&str>,
        history: &[Message],
    ) -> Result<Value, Error>;

    /// ストリーミングHTTPリクエストを実行
    /// 
    /// # Arguments
    /// * `request_json` - リクエストJSON文字列
    /// * `callback` - テキストチャンクを受け取るコールバック関数
    /// 
    /// # Returns
    /// * `Ok(())` - 成功
    /// * `Err(Error)` - エラーメッセージと終了コード
    fn make_http_streaming_request(
        &self,
        request_json: &str,
        callback: Box<dyn Fn(&str) -> Result<(), Error>>,
    ) -> Result<(), Error>;

    /// ストリームを LlmEvent 列に正規化してコールバックに渡す（デフォルト実装はテキストのみ TextDelta + Completed、チャンク受信ごとに即コールバック）
    fn stream_events(
        &self,
        request_json: &str,
        callback: &mut dyn FnMut(LlmEvent) -> Result<(), Error>,
    ) -> Result<(), Error>;
}

/// ツール呼び出し1件（assistant が model に返す用）
#[derive(Debug, Clone)]
pub struct ToolCallSpec {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// メッセージ構造体（user / assistant / tool と tool_calls 対応）
#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// assistant がツールを呼んだ場合
    pub tool_calls: Option<Vec<ToolCallSpec>>,
    /// role が "tool" のとき、どの call_id への返答か
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }

    /// ツール呼び出し付き assistant（content は空でも可）
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<(String, String, Value)>,
    ) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: Some(
                tool_calls
                    .into_iter()
                    .map(|(id, name, args)| ToolCallSpec { id, name, args })
                    .collect(),
            ),
            tool_call_id: None,
        }
    }

    /// ツール結果（role = "tool"）
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_new() {
        let msg = Message::new("user", "Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there");
    }

    #[test]
    fn test_message_with_empty_content() {
        let msg = Message::new("user", "");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "");
    }

    #[test]
    fn test_message_with_multiline_content() {
        let content = "Line 1\nLine 2\nLine 3";
        let msg = Message::user(content);
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_message_clone() {
        let msg1 = Message::user("Hello");
        let msg2 = msg1.clone();
        assert_eq!(msg1.role, msg2.role);
        assert_eq!(msg1.content, msg2.content);
    }

    #[test]
    fn test_message_different_roles() {
        let roles = vec!["user", "assistant", "system", "function"];
        for role in roles {
            let msg = Message::new(role, "test");
            assert_eq!(msg.role, role);
            assert_eq!(msg.content, "test");
        }
    }

    #[test]
    fn test_message_long_content() {
        let long_content = "a".repeat(1000);
        let msg = Message::user(&long_content);
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content.len(), 1000);
    }
}

