//! ツール呼び出しと実行記録。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ToolName;

/// LLM が返したツール呼び出し（正規化済み）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: ToolName,
    pub arguments: Value,
}

/// 実行済みツール呼び出しの成否。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutedToolStatus {
    Ok,
    Error,
}

/// クライアント向け `tool_calls` 記録。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedToolCall {
    pub id: String,
    pub name: ToolName,
    pub arguments: Value,
    pub status: ExecutedToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ExecutedToolCall {
    pub fn ok(id: String, name: ToolName, arguments: Value, output: String) -> Self {
        Self {
            id,
            name,
            arguments,
            status: ExecutedToolStatus::Ok,
            output: Some(output),
            error: None,
            message: None,
        }
    }

    pub fn err(
        id: String,
        name: ToolName,
        arguments: Value,
        error: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name,
            arguments,
            status: ExecutedToolStatus::Error,
            output: None,
            error: Some(error.into()),
            message: Some(message.into()),
        }
    }
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
    use crate::domain::ToolName;

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = ToolCall {
            id: "c1".into(),
            name: ToolName::read_file(),
            arguments: serde_json::json!({"path": "a.md"}),
        };
        let json = serde_json::to_string(&tc).expect("serialize");
        assert!(json.contains(r#""name":"read_file""#));
        let back: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, tc);
    }

    #[test]
    fn executed_tool_call_serde_roundtrip() {
        let tc = ExecutedToolCall::ok(
            "c1".into(),
            ToolName::shell_exec(),
            serde_json::json!({"command": "echo"}),
            "hi".into(),
        );
        let json = serde_json::to_string(&tc).expect("serialize");
        assert!(json.contains(r#""name":"shell_exec""#));
        let back: ExecutedToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, tc.name);
        assert_eq!(back.output, tc.output);
    }
}
