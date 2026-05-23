//! クライアント → aibe リクエスト。

use serde::{Deserialize, Serialize};

use crate::domain::ChatMessage;

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

/// クライアントが渡す付加コンテキスト。
#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
pub struct RequestContext {
    #[serde(default)]
    pub shell_log_tail: Option<String>,
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
            }
            _ => panic!("expected agent_turn"),
        }
    }
}
