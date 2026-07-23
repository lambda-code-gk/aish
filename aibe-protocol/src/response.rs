//! aibe → クライアント レスポンス（wire DTO）。

use serde::{Deserialize, Serialize};

use crate::executed_tool::{ExecutedToolCall, ToolRiskClass};
use crate::memory::{
    MemoryApplyStatus, MemoryChangeEventDto, MemoryEntryDto, MemoryKindDefinitionDto,
    MemoryQueryDto, MemoryQueryStatus, MemoryRecipeProposalDto, MemoryRecipeStatus,
    MemorySubscribeStatus,
};
use crate::work::{WorkApplyResponseBody, WorkQueryResponseBody};
use crate::{ClientToolErrorKind, ClientToolResultStatus};

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completion_report: Option<CompletionReport>,
    },
    /// `shell_exec` 実行前にクライアントへ yes/no を求める。
    ShellExecApprovalPrompt {
        id: String,
        turn_id: String,
        tool_call_id: String,
        command: String,
        args: Vec<String>,
    },
    /// write-like tool 実行前にクライアントへ yes/no を求める。
    ToolApprovalPrompt {
        id: String,
        turn_id: String,
        tool_call_id: String,
        tool_name: String,
        risk_class: ToolRiskClass,
        summary: String,
        paths: Vec<String>,
        preview: String,
        preview_truncated: bool,
    },
    ClientToolCallRequested {
        id: String,
        turn_id: String,
        call_id: String,
        name: String,
        arguments: serde_json::Value,
    },
    HumanTaskExecutionRequest {
        id: String,
        turn_id: String,
        tool_call_id: String,
        request: crate::HumanTaskRequest,
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
    MemoryKindListResult {
        id: String,
        status: MemoryQueryStatus,
        kinds: Vec<MemoryKindDefinitionDto>,
    },
    MemoryRecipeRunResult {
        id: String,
        status: MemoryRecipeStatus,
        summary: String,
        proposals: Vec<MemoryRecipeProposalDto>,
        applied_entries: Vec<MemoryEntryDto>,
    },
    MemorySubscribeResult {
        id: String,
        status: MemorySubscribeStatus,
        memory_space_id: String,
    },
    MemoryChanged {
        id: String,
        memory_space_id: String,
        event: MemoryChangeEventDto,
    },
    WorkApplyResult(WorkApplyResponseBody),
    WorkQueryResult(WorkQueryResponseBody),
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
    /// Human Task が Suspended で turn を終えた（本文 prefix ではなく typed outcome）。
    Suspended,
}

/// Task Completion の request-local 最終結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionReport {
    pub outcome: CompletionOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    pub criteria: Vec<CompletionCriterionReport>,
    pub unsatisfied_criteria: Vec<String>,
    pub unverified_items: Vec<String>,
    pub queries_used: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_terminal: Option<VerificationTerminal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<CompletionGapReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up_count: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationTerminal {
    Done,
    NeedsUser,
    Blocked,
    Stagnated,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionGapReport {
    pub criterion_id: String,
    pub observed: String,
    pub required_work: String,
    pub verification_plan_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOutcome {
    Done,
    NeedsUser,
    Blocked,
    BudgetExhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionCriterionReport {
    pub criterion_id: String,
    pub satisfied: bool,
    pub evidence: Vec<CompletionEvidenceReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_status: Option<CompletionCriterionStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionCriterionStatus {
    Satisfied,
    Unsatisfied,
    Unknown,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionEvidenceReport {
    pub evidence_id: String,
    pub source: CompletionEvidenceSource,
    pub summary: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionEvidenceSource {
    Tool,
    UnknownShellEffect,
    Observation,
    Verification,
    Deliverable,
    AgentTask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteTurnStatus {
    Ok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressPhase {
    Preparing,
    Routing,
    Thinking,
    Generating,
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

/// `route_turn` が返す「機能実行の構造化提案」。
///
/// MVP では side effect を持つ action は定義しても実行しない（もしくは承認経路のみ）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeatureAction {
    /// contextual memory を read-only に読みにいく提案。
    MemoryQuery {
        #[serde(default)]
        query: MemoryQueryDto,
    },
    /// memory recipe を read-only に提案するための提案。
    ///
    /// MVP では `apply=false` のみが実行対象。
    MemoryRecipeRun {
        recipe_id: String,
        #[serde(default)]
        apply: bool,
    },
    /// 会話用コンテキストとして log tail bytes を増やす提案。
    SetLogTailBytes { bytes: u64 },
    /// 使用する tools の提案（safe 判定は ai 側で再評価）。
    SetRecommendedTools { tools: Vec<String> },
    /// 将来拡張で未対応になった action。
    #[serde(other)]
    Unsupported,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub feature_actions: Vec<FeatureAction>,
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

impl ClientToolResultStatus {
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl ClientToolErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotInAishShell => "not_in_aish_shell",
            Self::SessionDirMissing => "session_dir_missing",
            Self::LogFileMissing => "log_file_missing",
            Self::SpanNotFound => "span_not_found",
            Self::SpanIncomplete => "span_incomplete",
            Self::InvalidArguments => "invalid_arguments",
            Self::OutputTooLarge => "output_too_large",
            Self::ToolNotSupported => "tool_not_supported",
            Self::ToolNotAllowed => "tool_not_allowed",
            Self::ToolTimeout => "tool_timeout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentTurnStatus, ClientToolResult, ToolName};

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
                feature_actions: vec![],
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
    fn route_turn_result_deserializes_without_feature_actions() {
        let json = r#"{
            "type":"route_turn_result",
            "id":"route-1",
            "status":"ok",
            "plan":{
                "conversation_id":"conv-1",
                "new_conversation":false,
                "route_kind":"chat",
                "recommended_preset":"fast",
                "recommended_tools":["read_file"],
                "log_tail_bytes":128,
                "require_shell_approval":true,
                "log_tail_escalation":false,
                "route_reason":"continue"
            }
        }"#;
        let back: ClientResponse = serde_json::from_str(json).expect("deserialize");
        match back {
            ClientResponse::RouteTurnResult { plan, .. } => {
                assert!(plan.feature_actions.is_empty());
            }
            other => panic!("unexpected response: {other:?}"),
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
            completion_report: None,
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
                ..
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

    #[test]
    fn client_tool_call_and_result_roundtrip() {
        let request = ClientResponse::ClientToolCallRequested {
            id: "call-1".into(),
            turn_id: "turn-1".into(),
            call_id: "tool-1".into(),
            name: "aish.replay_show".into(),
            arguments: serde_json::json!({
                "index": 1,
                "stream": "both",
                "tail_bytes": 8192
            }),
        };
        let request_json = serde_json::to_string(&request).expect("serialize request");
        let back_request: ClientResponse =
            serde_json::from_str(&request_json).expect("deserialize request");
        assert!(matches!(
            back_request,
            ClientResponse::ClientToolCallRequested { .. }
        ));

        let result = ClientToolResult {
            id: "call-1".into(),
            turn_id: "turn-1".into(),
            call_id: "tool-1".into(),
            status: ClientToolResultStatus::Ok,
            error_kind: None,
            content: "[untrusted terminal output]\nhello\n".into(),
        };
        let result_json = serde_json::to_string(&result).expect("serialize result");
        let back_result: ClientToolResult =
            serde_json::from_str(&result_json).expect("deserialize result");
        assert_eq!(back_result.status, ClientToolResultStatus::Ok);
        assert!(back_result
            .content
            .starts_with("[untrusted terminal output]"));
    }

    #[test]
    fn client_tool_call_requested_roundtrip() {
        let resp = ClientResponse::ClientToolCallRequested {
            id: "call-1".into(),
            turn_id: "turn-1".into(),
            call_id: "tool-1".into(),
            name: "aish.replay_show".into(),
            arguments: serde_json::json!({
                "index": 1,
                "stream": "both",
                "tail_bytes": 8192
            }),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::ClientToolCallRequested {
                id,
                turn_id,
                call_id,
                name,
                arguments,
            } => {
                assert_eq!(id, "call-1");
                assert_eq!(turn_id, "turn-1");
                assert_eq!(call_id, "tool-1");
                assert_eq!(name, "aish.replay_show");
                assert_eq!(arguments["stream"], "both");
            }
            _ => panic!("expected client_tool_call_requested"),
        }
    }

    #[test]
    fn client_tool_result_roundtrip() {
        let result = ClientToolResult {
            id: "call-1".into(),
            turn_id: "turn-1".into(),
            call_id: "tool-1".into(),
            status: ClientToolResultStatus::Ok,
            error_kind: None,
            content: "[untrusted terminal output]\nhello\n".into(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let back: ClientToolResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.status, ClientToolResultStatus::Ok);
        assert!(back.content.starts_with("[untrusted terminal output]"));
    }

    #[test]
    fn client_tool_error_kinds_roundtrip() {
        for kind in [
            ClientToolErrorKind::NotInAishShell,
            ClientToolErrorKind::SessionDirMissing,
            ClientToolErrorKind::LogFileMissing,
            ClientToolErrorKind::SpanNotFound,
            ClientToolErrorKind::SpanIncomplete,
            ClientToolErrorKind::InvalidArguments,
            ClientToolErrorKind::OutputTooLarge,
            ClientToolErrorKind::ToolNotSupported,
            ClientToolErrorKind::ToolNotAllowed,
            ClientToolErrorKind::ToolTimeout,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize");
            let back: ClientToolErrorKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, kind);
            assert!(!kind.as_str().is_empty());
        }
    }

    #[test]
    fn memory_subscribe_result_roundtrip() {
        let resp = ClientResponse::MemorySubscribeResult {
            id: "sub1".into(),
            status: MemorySubscribeStatus::Ok,
            memory_space_id: "ctx_a".into(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"memory_subscribe_result""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::MemorySubscribeResult {
                id,
                memory_space_id,
                ..
            } => {
                assert_eq!(id, "sub1");
                assert_eq!(memory_space_id, "ctx_a");
            }
            _ => panic!("expected memory_subscribe_result"),
        }
    }

    #[test]
    fn memory_changed_roundtrip() {
        use crate::memory::{
            MemoryChangeEventDto, MemoryChangeKind, MemoryEntryDto, MemoryInjectPolicyDto,
            MemoryScopeDto, MemoryStatusDto,
        };

        let resp = ClientResponse::MemoryChanged {
            id: "sub1".into(),
            memory_space_id: "ctx_a".into(),
            event: MemoryChangeEventDto {
                kind: "goal".into(),
                change: MemoryChangeKind::Added,
                entries: vec![MemoryEntryDto {
                    id: "mem_01".into(),
                    memory_space_id: "ctx_a".into(),
                    created_session_id: "s-1".into(),
                    last_session_id: "s-1".into(),
                    kind: "goal".into(),
                    scope: MemoryScopeDto::Project,
                    inject: MemoryInjectPolicyDto::Pinned,
                    status: MemoryStatusDto::Active,
                    text: "ship".into(),
                    project_key: None,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    version: 1,
                }],
            },
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains(r#""type":"memory_changed""#));
        let back: ClientResponse = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientResponse::MemoryChanged { event, .. } => {
                assert_eq!(event.kind, "goal");
                assert_eq!(event.change, MemoryChangeKind::Added);
            }
            _ => panic!("expected memory_changed"),
        }
    }
}
