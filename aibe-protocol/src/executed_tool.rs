//! クライアント向け `tool_calls` 記録（wire）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub name: String,
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
    pub fn ok(id: String, name: impl Into<String>, arguments: Value, output: String) -> Self {
        Self {
            id,
            name: name.into(),
            arguments,
            status: ExecutedToolStatus::Ok,
            output: Some(output),
            error: None,
            message: None,
        }
    }

    pub fn err(
        id: String,
        name: impl Into<String>,
        arguments: Value,
        error: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            arguments,
            status: ExecutedToolStatus::Error,
            output: None,
            error: Some(error.into()),
            message: Some(message.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolName;

    #[test]
    fn executed_tool_call_serde_roundtrip() {
        let tc = ExecutedToolCall::ok(
            "c1".into(),
            ToolName::shell_exec().to_string(),
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
