//! aibe → クライアント レスポンス（wire DTO）。

use serde::{Deserialize, Serialize};

use crate::executed_tool::ExecutedToolCall;
use crate::memory::{MemoryApplyStatus, MemoryEntryDto, MemoryQueryStatus};

/// NDJSON 1 行のレスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientResponse {
    Pong {
        id: String,
    },
    RouteTurnResult {
        id: String,
        status: RouteTurnStatus,
        plan: RoutePlan,
    },
    Progress {
        id: String,
        phase: ProgressPhase,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    AssistantStreaming {
        id: String,
        delta: String,
    },
    AgentTurnResult {
        id: String,
        status: AgentTurnStatus,
        assistant_message: ProtocolMessageOut,
        tool_calls: Vec<ExecutedToolCall>,
    },
    /// `shell_exec` 実行前にクライアントへ yes/no を求める。
    ShellExecApprovalPrompt {
        id: String,
        turn_id: String,
        tool_call_id: String,
        command: String,
        args: Vec<String>,
    },
    Cancelled {
        id: String,
        turn_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    MemoryApplyResult {
        id: String,
        status: MemoryApplyStatus,
        entries: Vec<MemoryEntryDto>,
    },
    MemoryQueryResult {
        id: String,
        status: MemoryQueryStatus,
        entries: Vec<MemoryEntryDto>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_block: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTurnStatus {
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    Thinking,
    ToolCall,
    WaitingApproval,
    Finalizing,
    Cancelling,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessageOut {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    OneShot,
    Chat,
    Continue,
    ToolAssisted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutePlan {
    pub conversation_id: String,
    pub new_conversation: bool,
    pub route_kind: RouteKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail_bytes: Option<u64>,
    pub require_shell_approval: bool,
    pub log_tail_escalation: bool,
    pub route_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteTurnResult {
    pub id: String,
    pub status: RouteTurnStatus,
    pub plan: RoutePlan,
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
    fn route_turn_result_roundtrip() {
        let resp = ClientResponse::RouteTurnResult {
            id: "route-1".into(),
            status: RouteTurnStatus::Ok,
            plan: RoutePlan {
                conversation_id: "conv-1".into(),
                new_conversation: false,
                route_kind: RouteKind::Chat,
                recommended_preset: Some("fast".into()),
                recommended_tools: Some(vec!["read_file".into()]),
                log_tail_bytes: Some(128),
                require_shell_approval: true,
                log_tail_escalation: false,
                route_reason: "continue".into(),
                confidence: Some(0.8),
            },
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"route_turn_result""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::RouteTurnResult { id, status, plan } => {
                assert_eq!(id, "route-1");
                assert_eq!(status, RouteTurnStatus::Ok);
                assert_eq!(plan.conversation_id, "conv-1");
                assert_eq!(plan.route_kind, RouteKind::Chat);
            }
            _ => panic!("expected route_turn_result"),
        }
    }

    #[test]
    fn client_response_progress_roundtrip() {
        let resp = ClientResponse::Progress {
            id: "turn".into(),
            phase: ProgressPhase::Thinking,
            message: Some("working".into()),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"progress""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(
            matches!(back, ClientResponse::Progress { id, phase, message } if id == "turn" && phase == ProgressPhase::Thinking && message.as_deref() == Some("working"))
        );
    }

    #[test]
    fn client_response_streaming_roundtrip() {
        let resp = ClientResponse::AssistantStreaming {
            id: "turn".into(),
            delta: "hello".into(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"assistant_streaming""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(
            matches!(back, ClientResponse::AssistantStreaming { id, delta } if id == "turn" && delta == "hello")
        );
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

    #[test]
    fn client_response_cancelled_roundtrip() {
        let resp = ClientResponse::Cancelled {
            id: "c1".into(),
            turn_id: "t1".into(),
            reason: Some("user requested".into()),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"cancelled""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(
            matches!(back, ClientResponse::Cancelled { id, turn_id, reason } if id == "c1" && turn_id == "t1" && reason.as_deref() == Some("user requested"))
        );
    }
}
