//! aibe → クライアント レスポンス（wire DTO）。

use serde::{Deserialize, Serialize};

use crate::executed_tool::ExecutedToolCall;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentTurnStatus, ToolName};

    #[test]
    fn error_code_serde_roundtrip() {
        for code in [
            ErrorCode::InvalidRequest,
            ErrorCode::InternalError,
            ErrorCode::ProviderError,
            ErrorCode::ToolError,
            ErrorCode::ToolTimeout,
            ErrorCode::ToolNotAllowed,
            ErrorCode::MaxToolRounds,
        ] {
            let json = serde_json::to_string(&code).expect("serialize");
            let back: ErrorCode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, code);
        }
    }

    #[test]
    fn client_response_pong_roundtrip() {
        let resp = ClientResponse::Pong { id: "p1".into() };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"pong""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, ClientResponse::Pong { id } if id == "p1"));
    }

    #[test]
    fn client_response_agent_turn_result_roundtrip() {
        let resp = ClientResponse::AgentTurnResult {
            id: "t1".into(),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "hi".into(),
            },
            tool_calls: vec![ExecutedToolCall::ok(
                "c1".into(),
                ToolName::read_file(),
                serde_json::json!({"path": "a.md"}),
                "out".into(),
            )],
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"agent_turn_result""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::AgentTurnResult {
                id,
                status,
                assistant_message,
                tool_calls,
            } => {
                assert_eq!(id, "t1");
                assert_eq!(status, AgentTurnStatus::Ok);
                assert_eq!(assistant_message.content, "hi");
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "read_file");
            }
            _ => panic!("expected agent_turn_result"),
        }
    }

    #[test]
    fn client_response_error_roundtrip() {
        let resp = ClientResponse::error("e1".into(), ErrorCode::ToolNotAllowed, "not allowed");
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""code":"tool_not_allowed""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::Error { id, code, message } => {
                assert_eq!(id, "e1");
                assert_eq!(code, ErrorCode::ToolNotAllowed);
                assert_eq!(message, "not allowed");
            }
            _ => panic!("expected error"),
        }
    }
}
