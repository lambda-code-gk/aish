//! クライアント → aibe リクエスト（wire DTO）。

use serde::{Deserialize, Serialize};

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        llm_profile: Option<String>,
    },
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
}
