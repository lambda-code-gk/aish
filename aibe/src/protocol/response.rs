//! aibe → クライアント レスポンス。

use serde::{Deserialize, Serialize};

use crate::domain::{ChatMessage, ExecutedToolCall};

/// NDJSON 1 行のレスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientResponse {
    Pong {
        id: String,
    },
    AgentTurnResult {
        id: String,
        status: AgentTurnStatus,
        assistant_message: ProtocolMessageOut,
        tool_calls: Vec<ExecutedToolCall>,
    },
    Error {
        id: String,
        code: ErrorCode,
        message: String,
    },
}

/// `agent_turn_result.status` の取りうる値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTurnStatus {
    Ok,
    MaxToolRounds,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessageOut {
    pub role: String,
    pub content: String,
}

impl From<ChatMessage> for ProtocolMessageOut {
    fn from(m: ChatMessage) -> Self {
        Self::from_assistant(&m)
    }
}

impl ProtocolMessageOut {
    pub fn from_assistant(m: &ChatMessage) -> Self {
        Self {
            role: m.role.to_string(),
            content: m.content.clone(),
        }
    }
}

impl ClientResponse {
    pub fn error(id: String, code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Error {
            id,
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    InternalError,
    ProviderError,
    ToolError,
    ToolTimeout,
    ToolNotAllowed,
    MaxToolRounds,
}
