//! クライアント → aibe リクエスト。

use serde::{Deserialize, Serialize};

use crate::domain::{AgentTurnContext, ChatMessage, ClientCwd, ShellLogTail};

/// NDJSON 1 行のリクエスト。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientRequest {
    Ping {
        id: String,
    },
    AgentTurn {
        id: String,
        messages: Vec<ProtocolMessage>,
        #[serde(default)]
        tools: Vec<String>,
        #[serde(default)]
        context: RequestContext,
    },
}

/// プロトコル上のメッセージ（serde 用）。
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ProtocolMessage {
    pub role: String,
    pub content: String,
}

impl From<ProtocolMessage> for ChatMessage {
    fn from(m: ProtocolMessage) -> Self {
        ChatMessage {
            role: m.role,
            content: m.content,
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

/// クライアントが渡す付加コンテキスト（wire DTO。domain 変換は `TryFrom` のみ）。
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct RequestContext {
    #[serde(default)]
    pub shell_log_tail: Option<String>,
    /// クライアントのカレントディレクトリ（絶対パス）。ツール有効時は必須。
    #[serde(default)]
    pub cwd: Option<String>,
}

/// protocol DTO → domain 変換エラー。
///
/// 現在の wire 正規化（tail truncate・相対 cwd の `None` 化）は失敗しない。
/// `TryFrom` を唯一の変換経路にし、`From` は意図的に実装しない。
#[derive(Debug)]
pub enum RequestContextConversionError {}

// 0005 仕様: protocol → domain 変換は `TryFrom` に閉じる（`From` による暗黙変換を禁止）。
#[allow(clippy::infallible_try_from)]
impl TryFrom<RequestContext> for AgentTurnContext {
    type Error = RequestContextConversionError;

    fn try_from(ctx: RequestContext) -> Result<AgentTurnContext, Self::Error> {
        let client_cwd = ctx
            .cwd
            .as_deref()
            .and_then(|raw| ClientCwd::parse(raw).ok());
        let shell_log_tail = ctx
            .shell_log_tail
            .as_deref()
            .and_then(ShellLogTail::from_wire_opt);
        Ok(AgentTurnContext {
            client_cwd,
            shell_log_tail,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_turn_deserializes_defaults() {
        let req: ClientRequest = serde_json::from_str(
            r#"{"type":"agent_turn","id":"1","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .expect("parse");
        match req {
            ClientRequest::AgentTurn { tools, context, .. } => {
                assert!(tools.is_empty());
                assert!(context.shell_log_tail.is_none());
                assert!(context.cwd.is_none());
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
    fn try_from_relative_cwd_becomes_none() {
        let ctx = RequestContext {
            cwd: Some("relative/path".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert!(domain.client_cwd.is_none());
    }

    #[test]
    fn try_from_absolute_cwd_parses() {
        let ctx = RequestContext {
            cwd: Some("/tmp/proj".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert_eq!(
            domain.client_cwd.expect("cwd").as_path(),
            std::path::Path::new("/tmp/proj")
        );
    }

    #[test]
    fn try_from_empty_tail_becomes_none() {
        let ctx = RequestContext {
            shell_log_tail: Some("".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert!(domain.shell_log_tail.is_none());
    }

    #[test]
    fn try_from_whitespace_tail_becomes_none() {
        let ctx = RequestContext {
            shell_log_tail: Some("  \n\t  ".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert!(domain.shell_log_tail.is_none());
    }
}
