//! ツール呼び出しと実行記録（server 内部）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// LLM が返したツール呼び出し（正規化済み）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    /// LLM が返した生のツール名（組み込み外の名前も保持する）。
    pub name: String,
    pub arguments: Value,
    /// Gemini 等の provider 固有 part。wire / protocol には載せない。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_extras: Option<Value>,
}

/// ツール実行結果（LLM 向け tool メッセージ用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::READ_FILE;

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = ToolCall {
            id: "c1".into(),
            name: READ_FILE.to_string(),
            arguments: serde_json::json!({"path": "a.md"}),
            provider_extras: None,
        };
        let json = serde_json::to_string(&tc).expect("serialize");
        assert!(json.contains(r#""name":"read_file""#));
        let back: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, tc);
    }
}
