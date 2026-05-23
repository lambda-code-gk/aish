//! ツール呼び出しと実行記録。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// LLM が返したツール呼び出し（正規化済み）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// クライアント向け `tool_calls` 記録。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ExecutedToolCall {
    pub fn ok(id: String, name: String, arguments: Value, output: String) -> Self {
        Self {
            id,
            name,
            arguments,
            status: "ok".to_string(),
            output: Some(output),
            error: None,
            message: None,
        }
    }

    pub fn err(
        id: String,
        name: String,
        arguments: Value,
        error: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name,
            arguments,
            status: "error".to_string(),
            output: None,
            error: Some(error.into()),
            message: Some(message.into()),
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("ExecutedToolCall serializes")
    }
}

/// ツール実行結果（LLM 向け tool メッセージ用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}
