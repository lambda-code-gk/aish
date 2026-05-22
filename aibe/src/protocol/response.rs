//! aibe → クライアント レスポンス。

use serde::{Deserialize, Serialize};

use crate::domain::ChatMessage;

/// NDJSON 1 行のレスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientResponse {
    Pong {
        id: String,
    },
    AgentTurnResult {
        id: String,
        status: String,
        assistant_message: ProtocolMessageOut,
        tool_calls: Vec<serde_json::Value>,
    },
    Error {
        id: String,
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessageOut {
    pub role: String,
    pub content: String,
}

impl From<ChatMessage> for ProtocolMessageOut {
    fn from(m: ChatMessage) -> Self {
        Self {
            role: m.role,
            content: m.content,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    InternalError,
    ProviderError,
}
