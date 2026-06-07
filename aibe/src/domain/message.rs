//! チャットメッセージ。

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ToolCall;

/// wire 上の未知 role。
#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown message role: {0}")]
pub struct UnknownMessageRole(pub String);

/// 会話メッセージの role（wire 互換: snake_case 文字列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

impl MessageRole {
    pub fn parse_wire(s: &str) -> Result<Self, UnknownMessageRole> {
        match s {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "tool" => Ok(Self::Tool),
            "system" => Ok(Self::System),
            other => Err(UnknownMessageRole(other.to_string())),
        }
    }

    pub fn as_wire(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// プロバイダへ渡す会話メッセージ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Some(tool_calls),
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
    }

    pub fn tool_error(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::tool(tool_call_id, content)
    }

    pub fn is_role(&self, role: MessageRole) -> bool {
        self.role == role
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wire_known_roles() {
        assert_eq!(MessageRole::parse_wire("user").unwrap(), MessageRole::User);
        assert_eq!(
            MessageRole::parse_wire("assistant").unwrap(),
            MessageRole::Assistant
        );
        assert_eq!(MessageRole::parse_wire("tool").unwrap(), MessageRole::Tool);
        assert_eq!(
            MessageRole::parse_wire("system").unwrap(),
            MessageRole::System
        );
    }

    #[test]
    fn parse_wire_unknown_role() {
        assert_eq!(
            MessageRole::parse_wire("moderator").unwrap_err(),
            UnknownMessageRole("moderator".into())
        );
    }

    #[test]
    fn chat_message_serde_roundtrip() {
        let msg = ChatMessage::user("hello");
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains("\"role\":\"user\""));
        let back: ChatMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, msg);
    }

    #[test]
    fn is_role_helper() {
        let msg = ChatMessage::assistant("hi");
        assert!(msg.is_role(MessageRole::Assistant));
        assert!(!msg.is_role(MessageRole::User));
    }
}
