//! クライアント → aibe リクエスト（wire DTO）。

use serde::{Deserialize, Serialize};

use crate::memory::{MemoryApplyRequestBody, MemoryQueryRequestBody};

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
    },
    /// contextual memory の書き込み。
    MemoryApply(MemoryApplyRequestBody),
    /// contextual memory の読み取り。
    MemoryQuery(MemoryQueryRequestBody),
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
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnSession {
    pub ai_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aish_session_dir: Option<String>,
    pub tty: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouteTurnConversation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_summary: Option<String>,
    pub new_conversation: bool,
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
            MemoryApplyRequestBody, MemoryContext, MemoryInjectPolicyDto, MemoryOperationDto,
            MemoryScopeDto, MemoryStatusDto,
        };

        let req = ClientRequest::MemoryApply(MemoryApplyRequestBody {
            id: "m1".into(),
            session_id: "sess-1".into(),
            context: MemoryContext {
                cwd: "/tmp/proj".into(),
            },
            operation: MemoryOperationDto::Add {
                kind: "goal".into(),
                scope: MemoryScopeDto::Project,
                inject: MemoryInjectPolicyDto::Pinned,
                status: MemoryStatusDto::Active,
                text: "ship it".into(),
                make_active: true,
            },
        });
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains(r#""type":"memory_apply""#));
        let back: ClientRequest = serde_json::from_str(&json).expect("deserialize");
        match back {
            ClientRequest::MemoryApply(body) => {
                assert_eq!(body.id, "m1");
                assert_eq!(body.session_id, "sess-1");
                assert_eq!(body.context.cwd, "/tmp/proj");
                assert!(matches!(
                    body.operation,
                    MemoryOperationDto::Add { kind, text, .. } if kind == "goal" && text == "ship it"
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
                cwd: "/tmp/proj".into(),
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
                assert_eq!(body.context.cwd, "/tmp/proj");
                assert_eq!(body.query.kind.as_deref(), Some("idea"));
                assert_eq!(body.query.limit, Some(10));
            }
            _ => panic!("expected memory_query"),
        }
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
}
