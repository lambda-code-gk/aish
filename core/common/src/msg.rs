//! 型付きメッセージ履歴（Msg）
//!
//! AgentLoop は Vec<Msg> を保持し、LLMアダプタが各APIのリクエスト形式に変換する。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会話メッセージ（システム・ユーザー・アシスタント・ツール呼び出し・ツール結果）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    System(String),
    User(String),
    Assistant(String),
    ToolCall {
        call_id: String,
        name: String,
        args: Value,
        /// Gemini 3 で必須の thought_signature
        thought_signature: Option<String>,
    },
    ToolResult {
        call_id: String,
        name: String,
        result: Value,
    },
}

impl Msg {
    pub fn system(s: impl Into<String>) -> Self {
        Msg::System(s.into())
    }
    pub fn user(s: impl Into<String>) -> Self {
        Msg::User(s.into())
    }
    pub fn assistant(s: impl Into<String>) -> Self {
        Msg::Assistant(s.into())
    }
    pub fn tool_call(call_id: impl Into<String>, name: impl Into<String>, args: Value, thought_signature: Option<String>) -> Self {
        Msg::ToolCall {
            call_id: call_id.into(),
            name: name.into(),
            args,
            thought_signature,
        }
    }
    pub fn tool_result(call_id: impl Into<String>, name: impl Into<String>, result: Value) -> Self {
        Msg::ToolResult {
            call_id: call_id.into(),
            name: name.into(),
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_user_assistant() {
        let u = Msg::user("Hi");
        let a = Msg::assistant("Hello");
        assert!(matches!(u, Msg::User(s) if s == "Hi"));
        assert!(matches!(a, Msg::Assistant(s) if s == "Hello"));
    }

    #[test]
    fn test_msg_tool_call_result() {
        let tc = Msg::tool_call("c1", "run_shell", serde_json::json!({"cmd": "ls"}), Some("sig123".to_string()));
        let tr = Msg::tool_result("c1", "run_shell", serde_json::json!({"ok": true}));
        assert!(matches!(tc, Msg::ToolCall { call_id, name, thought_signature, .. } if call_id == "c1" && name == "run_shell" && thought_signature == Some("sig123".to_string())));
        assert!(matches!(tr, Msg::ToolResult { call_id, name, .. } if call_id == "c1" && name == "run_shell"));
    }
}
