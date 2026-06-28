//! クライアント → aibe リクエスト（wire DTO）。

use serde::{Deserialize, Serialize};

use crate::memory::{
    MemoryApplyRequestBody, MemoryKindListRequestBody, MemoryQueryRequestBody,
    MemoryRecipeRunRequestBody, MemorySubscribeRequestBody,
};
use crate::work::{WorkApplyRequestBody, WorkQueryRequestBody};
use crate::ToolRiskClass;

/// NDJSON 1 行のリクエスト。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientRequest {
    Ping {
        id: String,
    },
    RouteTurn {
        id: String,
        query: String,
        cwd: String,
        session: RouteTurnSession,
        conversation: RouteTurnConversation,
        cli_overrides: RouteTurnCliOverrides,
    },
    AgentTurn {
        id: String,
        messages: Vec<ProtocolMessage>,
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        client_tools: Vec<ClientProvidedToolSpec>,
        #[serde(default)]
        context: RequestContext,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        llm_profile: Option<String>,
    },
    /// 進行中の turn を取り消す。
    CancelTurn {
        id: String,
        turn_id: String,
    },
    /// `shell_exec` 実行前承認の応答（同一 socket 接続上）。
    ShellExecApproval {
        id: String,
        turn_id: String,
        tool_call_id: String,
        approved: bool,
        approval_origin: ShellExecApprovalOrigin,
    },
    /// `client_tool` 実行結果（同一 socket 接続上）。
    ClientToolResult(ClientToolResult),
    /// contextual memory の書き込み。
    MemoryApply(MemoryApplyRequestBody),
    /// contextual memory の読み取り。
    MemoryQuery(MemoryQueryRequestBody),
    /// memory kind registry の一覧。
    MemoryKindList(MemoryKindListRequestBody),
    /// memory recipe の実行（LLM 提案 / 任意 apply）。
    MemoryRecipeRun(MemoryRecipeRunRequestBody),
    /// memory 変更の購読（専用接続。結果後に `MemoryChanged` を push）。
    MemorySubscribe(MemorySubscribeRequestBody),
    /// 作業文脈の原子的な更新。
    WorkApply(WorkApplyRequestBody),
    /// 作業文脈 snapshot の読み取り。
    WorkQuery(WorkQueryRequestBody),
}

/// `shell_exec` 承認の provenance。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellExecApprovalOrigin {
    UiYes,
    UiNo,
    UiAlwaysThisSessionExactInvocation,
    UiCommandOnly,
    SessionAllowed,
    SessionCacheExactInvocation,
    SessionCacheCommandName,
    PatternReadOnly,
    PatternMutating,
}

/// プロトコル上のメッセージ（serde 用）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolMessage {
    pub role: String,
    pub content: String,
}

/// クライアントが渡す付加コンテキスト（wire DTO）。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RequestContext {
    #[serde(default)]
    pub shell_log_tail: Option<String>,
    /// クライアントのカレントディレクトリ（絶対パス）。ツール有効時は必須。
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// この turn のみ LLM に前置する system 本文。クライアントが組み立て、aibe は注入のみ行う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<String>,
    /// クライアントが解決済みの contextual memory space。注入時の解決順 1 位（0035）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_space_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientProvidedToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub risk_class: ToolRiskClass,
    pub max_output_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientToolResultStatus {
    Ok,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientToolErrorKind {
    NotInAishShell,
    SessionDirMissing,
    LogFileMissing,
    SpanNotFound,
    SpanIncomplete,
    InvalidArguments,
    OutputTooLarge,
    ToolNotSupported,
    ToolNotAllowed,
    ToolTimeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientToolResult {
    pub id: String,
    pub turn_id: String,
    pub call_id: String,
    pub status: ClientToolResultStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<ClientToolErrorKind>,
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnSession {
    pub ai_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aish_session_dir: Option<String>,
    pub tty: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnPreprocessorHints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_needs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preprocessor_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preprocessor_reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_bps: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_requires_approval: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnConversation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_summary: Option<String>,
    pub new_conversation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preprocessor_hints: Option<RouteTurnPreprocessorHints>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnCliOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail_bytes: Option<u64>,
    #[serde(default)]
    pub yes_exec: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_turn_deserializes_llm_profile() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","llm_profile":"fast","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { llm_profile, .. } => {
                assert_eq!(llm_profile.as_deref(), Some("fast"));
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn agent_turn_deserializes_defaults() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn {
                tools,
                context,
                llm_profile,
                ..
            } => {
                assert!(tools.is_empty());
                assert!(context.shell_log_tail.is_none());
                assert!(context.cwd.is_none());
                assert!(llm_profile.is_none());
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn agent_turn_deserializes_context_system_instruction() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}],"context":{"system_instruction":"be brief"}}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { context, .. } => {
                assert_eq!(context.system_instruction.as_deref(), Some("be brief"));
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn agent_turn_deserializes_context_memory_space_id() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}],"context":{"memory_space_id":"ctx_a"}}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { context, .. } => {
                assert_eq!(context.memory_space_id.as_deref(), Some("ctx_a"));
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn agent_turn_context_omits_memory_space_id_when_none() {
        let context = RequestContext::default();
        let json = serde_json::to_string(&context).expect("serialize");
        assert!(!json.contains("memory_space_id"));
    }

    #[test]
    fn agent_turn_deserializes_context_cwd() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}],"context":{"cwd":"/tmp/proj"}}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { context, .. } => {
                assert_eq!(context.cwd.as_deref(), Some("/tmp/proj"));
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn route_turn_conversation_roundtrip_with_preprocessor_hints() {
        let req = ClientRequest::RouteTurn {
            id: "r1".into(),
            query: "git diff".into(),
            cwd: "/tmp/proj".into(),
            session: RouteTurnSession::default(),
            conversation: RouteTurnConversation {
                conversation_id: None,
                recent_summary: None,
                new_conversation: true,
                preprocessor_hints: Some(RouteTurnPreprocessorHints {
                    context_needs: vec!["git_status".into(), "git_diff".into()],
                    tool_hints: vec!["git_status".into()],
                    failure_kind: None,
                    preprocessor_intent: Some("inspect".into()),
                    preprocessor_reason_codes: vec!["git_context".into()],
                    ..Default::default()
                }),
            },
            cli_overrides: RouteTurnCliOverrides::default(),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::RouteTurn { conversation, .. } => {
                let hints = conversation.preprocessor_hints.expect("preprocessor_hints");
                assert_eq!(hints.context_needs.len(), 2);
                assert_eq!(hints.preprocessor_intent.as_deref(), Some("inspect"));
            }
            _ => panic!("expected route_turn"),
        }
    }

    #[test]
    fn route_turn_preprocessor_hints_deserialize_legacy_without_confidence_fields() {
        let legacy = r#"{
            "context_needs":["git_status"],
            "tool_hints":["git_status"],
            "preprocessor_intent":"inspect"
        }"#;
        let hints: RouteTurnPreprocessorHints = serde_json::from_str(legacy).expect("legacy");
        assert_eq!(hints.context_needs, vec!["git_status"]);
        assert!(hints.confidence_bps.is_none());
        assert!(hints.confidence_gate.is_none());
        assert!(hints.safety_requires_approval.is_none());
    }

    #[test]
    fn route_turn_preprocessor_hints_roundtrip_confidence_fields() {
        let hints = RouteTurnPreprocessorHints {
            context_needs: vec!["vcs_status".into()],
            tool_hints: vec!["git_status".into()],
            failure_kind: None,
            preprocessor_intent: Some("inspect".into()),
            preprocessor_reason_codes: vec!["vcs_context".into()],
            confidence_bps: Some(7200),
            confidence_gate: Some("assist_route_turn".into()),
            safety_requires_approval: Some(false),
        };
        let json = serde_json::to_string(&hints).expect("serialize");
        let back: RouteTurnPreprocessorHints = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.confidence_bps, Some(7200));
        assert_eq!(back.confidence_gate.as_deref(), Some("assist_route_turn"));
        assert_eq!(back.safety_requires_approval, Some(false));
    }

    #[test]
    fn route_turn_hints_serialize_as_additive_optional_field() {
        let conversation = RouteTurnConversation {
            conversation_id: None,
            recent_summary: None,
            new_conversation: true,
            preprocessor_hints: None,
        };
        let json = serde_json::to_string(&conversation).expect("serialize");
        assert!(!json.contains("preprocessor_hints"));
        let legacy = r#"{"new_conversation":true}"#;
        let parsed: RouteTurnConversation = serde_json::from_str(legacy).expect("legacy");
        assert!(parsed.preprocessor_hints.is_none());
    }

    #[test]
    fn route_turn_roundtrip() {
        let req = ClientRequest::RouteTurn {
            id: "r1".into(),
            query: "hello".into(),
            cwd: "/tmp/proj".into(),
            session: RouteTurnSession {
                ai_session_id: "session-1".into(),
                aish_session_dir: Some("/tmp/aish".into()),
                tty: true,
            },
            conversation: RouteTurnConversation {
                conversation_id: Some("conv-1".into()),
                recent_summary: Some("latest".into()),
                new_conversation: false,
                preprocessor_hints: None,
            },
            cli_overrides: RouteTurnCliOverrides {
                preset: Some("fast".into()),
                tools: Some(vec!["read_file".into()]),
                log_tail_bytes: Some(128),
                yes_exec: true,
            },
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::RouteTurn {
                id,
                query,
                cwd,
                session,
                conversation,
                cli_overrides,
            } => {
                assert_eq!(id, "r1");
                assert_eq!(query, "hello");
                assert_eq!(cwd, "/tmp/proj");
                assert_eq!(session.ai_session_id, "session-1");
                assert_eq!(conversation.conversation_id.as_deref(), Some("conv-1"));
                assert_eq!(cli_overrides.tools.as_ref().map(Vec::len), Some(1));
            }
            _ => panic!("expected route_turn"),
        }
    }

    #[test]
    fn agent_turn_client_tools_roundtrip() {
        let req = ClientRequest::AgentTurn {
            id: "t1".into(),
            messages: vec![ProtocolMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            tools: vec![],
            client_tools: vec![ClientProvidedToolSpec {
                name: "aish.replay_show".into(),
                description: "show replay".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "index": { "type": "integer" }
                    },
                    "required": ["index"]
                }),
                risk_class: ToolRiskClass::ReadOnly,
                max_output_bytes: 8192,
            }],
            context: RequestContext {
                cwd: Some("/tmp/proj".into()),
                ..Default::default()
            },
            llm_profile: Some("fast".into()),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::AgentTurn {
                client_tools,
                llm_profile,
                ..
            } => {
                assert_eq!(client_tools.len(), 1);
                assert_eq!(client_tools[0].name, "aish.replay_show");
                assert_eq!(llm_profile.as_deref(), Some("fast"));
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn agent_turn_client_tools_default_to_empty() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { client_tools, .. } => {
                assert!(client_tools.is_empty());
            }
            _ => panic!("expected agent_turn"),
        }
    }

    #[test]
    fn shell_exec_approval_roundtrip_with_origin() {
        let req = ClientRequest::ShellExecApproval {
            id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            approved: true,
            approval_origin: ShellExecApprovalOrigin::UiAlwaysThisSessionExactInvocation,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""approval_origin":"ui_always_this_session_exact_invocation""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(
            back,
            ClientRequest::ShellExecApproval {
                approved: true,
                approval_origin: ShellExecApprovalOrigin::UiAlwaysThisSessionExactInvocation,
                ..
            }
        ));
    }

    #[test]
    fn cancel_turn_roundtrip() {
        let req = ClientRequest::CancelTurn {
            id: "c1".into(),
            turn_id: "t1".into(),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"cancel_turn""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        assert!(
            matches!(back, ClientRequest::CancelTurn { id, turn_id } if id == "c1" && turn_id == "t1")
        );
    }

    #[test]
    fn memory_apply_roundtrip() {
        use crate::memory::{
            MemoryApplyRequestBody, MemoryContext, MemoryInjectPolicyDto, MemoryOperationAdd,
            MemoryOperationDto, MemoryScopeDto, MemoryStatusDto,
        };

        let req = ClientRequest::MemoryApply(MemoryApplyRequestBody {
            id: "m1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: Some("/tmp/proj".into()),
                memory_space_id: None,
            },
            operation: MemoryOperationDto::Add(MemoryOperationAdd {
                kind: "goal".into(),
                scope: Some(MemoryScopeDto::Project),
                inject: Some(MemoryInjectPolicyDto::Pinned),
                status: Some(MemoryStatusDto::Active),
                text: "ship it".into(),
                make_active: Some(true),
            }),
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_apply""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemoryApply(body) => {
                assert_eq!(body.id, "m1");
                assert_eq!(body.session_id, "sess-1");
                assert_eq!(body.context.cwd.as_deref(), Some("/tmp/proj"));
                assert!(matches!(
                    body.operation,
                    MemoryOperationDto::Add(add) if add.kind == "goal" && add.text == "ship it"
                ));
            }
            _ => panic!("expected memory_apply"),
        }
    }

    #[test]
    fn memory_query_roundtrip() {
        use crate::memory::{
            MemoryContext, MemoryQueryDto, MemoryQueryRequestBody, MemoryScopeDto, MemoryStatusDto,
        };

        let req = ClientRequest::MemoryQuery(MemoryQueryRequestBody {
            id: "q1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: Some("/tmp/proj".into()),
                memory_space_id: None,
            },
            query: MemoryQueryDto {
                kind: Some("idea".into()),
                scope: Some(MemoryScopeDto::Project),
                status: Some(MemoryStatusDto::Open),
                active_only: false,
                include_archived: false,
                limit: Some(10),
                include_prompt_block: false,
                user_query: None,
            },
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_query""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemoryQuery(body) => {
                assert_eq!(body.id, "q1");
                assert_eq!(body.session_id, "sess-1");
                assert_eq!(body.context.cwd.as_deref(), Some("/tmp/proj"));
                assert_eq!(body.query.kind.as_deref(), Some("idea"));
                assert_eq!(body.query.limit, Some(10));
            }
            _ => panic!("expected memory_query"),
        }
    }

    #[test]
    fn memory_apply_rejects_unknown_operation_fields() {
        let json = r#"{
            "type": "memory_apply",
            "id": "m1",
            "session_id": "sess",
            "context": { "cwd": "/tmp" },
            "operation": {
                "op": "add",
                "kind": "goal",
                "scope": "project",
                "inject": "pinned",
                "status": "active",
                "text": "x",
                "make_active": true,
                "unknown": true
            }
        }"#;
        assert!(serde_json::from_str::<ClientRequest>(json).is_err());
    }

    #[test]
    fn memory_apply_rejects_unknown_top_level_fields() {
        let json = r#"{
            "type": "memory_apply",
            "id": "m1",
            "session_id": "sess",
            "context": { "cwd": "/tmp" },
            "operation": {
                "op": "add",
                "kind": "goal",
                "scope": "project",
                "inject": "pinned",
                "status": "active",
                "text": "x",
                "make_active": true
            },
            "project_key": "/tmp"
        }"#;
        assert!(serde_json::from_str::<ClientRequest>(json).is_err());
    }

    #[test]
    fn memory_query_rejects_unknown_context_fields() {
        let json = r#"{
            "type": "memory_query",
            "id": "q1",
            "session_id": "sess",
            "context": { "cwd": "/tmp", "project_key": "/tmp" },
            "query": {}
        }"#;
        assert!(serde_json::from_str::<ClientRequest>(json).is_err());
    }

    #[test]
    fn memory_kind_list_request_roundtrip() {
        use crate::memory::{MemoryContext, MemoryKindListRequestBody};

        let req = ClientRequest::MemoryKindList(MemoryKindListRequestBody {
            id: "k1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: Some("/tmp/proj".into()),
                memory_space_id: Some("ctx_a".into()),
            },
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_kind_list""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemoryKindList(body) => {
                assert_eq!(body.id, "k1");
                assert_eq!(body.session_id, "sess-1");
            }
            _ => panic!("expected memory_kind_list"),
        }
    }

    #[test]
    fn memory_recipe_run_request_roundtrip() {
        use crate::memory::{MemoryContext, MemoryRecipeRunRequestBody};

        let req = ClientRequest::MemoryRecipeRun(MemoryRecipeRunRequestBody {
            id: "r1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: Some("/tmp/proj".into()),
                memory_space_id: Some("ctx_a".into()),
            },
            recipe: "clarify-goal".into(),
            apply: true,
            user_instruction: Some("focus MVP".into()),
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_recipe_run""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemoryRecipeRun(body) => {
                assert_eq!(body.recipe, "clarify-goal");
                assert!(body.apply);
            }
            _ => panic!("expected memory_recipe_run"),
        }
    }

    #[test]
    fn memory_subscribe_request_roundtrip() {
        use crate::memory::{MemoryContext, MemorySubscribeRequestBody};

        let req = ClientRequest::MemorySubscribe(MemorySubscribeRequestBody {
            id: "sub1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: Some("/tmp/proj".into()),
                memory_space_id: Some("ctx_a".into()),
            },
            kind: Some("goal".into()),
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_subscribe""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemorySubscribe(body) => {
                assert_eq!(body.id, "sub1");
                assert_eq!(body.kind.as_deref(), Some("goal"));
            }
            _ => panic!("expected memory_subscribe"),
        }
    }
}
