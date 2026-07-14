//! wire DTO ↔ domain 変換（`aibe-protocol` → `domain`）。

use std::fmt;

use aibe_protocol::{
    ProtocolMessage, ProtocolMessageOut, RequestContext, SYSTEM_INSTRUCTION_MAX_BYTES,
};

use crate::domain::{AgentTurnContext, ChatMessage, ClientCwd, MessageRole, ShellLogTail};

/// protocol メッセージ → domain 変換エラー。
#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolMessageConversionError {
    UnknownRole { role: String },
}

impl fmt::Display for ProtocolMessageConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRole { role } => write!(f, "unknown message role: {role}"),
        }
    }
}

impl TryFrom<ProtocolMessage> for ChatMessage {
    type Error = ProtocolMessageConversionError;

    fn try_from(m: ProtocolMessage) -> Result<Self, Self::Error> {
        let role = MessageRole::parse_wire(&m.role)
            .map_err(|_| ProtocolMessageConversionError::UnknownRole { role: m.role })?;
        Ok(ChatMessage {
            role,
            content: m.content,
            tool_call_id: None,
            tool_calls: None,
        })
    }
}

/// protocol DTO → domain 変換エラー（現在は infallible）。
#[derive(Debug)]
pub enum RequestContextConversionError {}

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
        let system_instruction = normalize_system_instruction(ctx.system_instruction);
        Ok(AgentTurnContext {
            client_cwd,
            shell_log_tail,
            system_instruction,
            ai_session_id: ctx.ai_session_id,
            memory_space_id: ctx.memory_space_id.filter(|s| !s.is_empty()),
            collaborative_handoff: ctx.collaborative_handoff,
            execution_mode: ctx.execution_mode,
        })
    }
}

fn normalize_system_instruction(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() > SYSTEM_INSTRUCTION_MAX_BYTES {
        let end = trimmed.floor_char_boundary(SYSTEM_INSTRUCTION_MAX_BYTES);
        Some(trimmed[..end].to_string())
    } else {
        Some(trimmed)
    }
}

pub fn protocol_message_out_from_chat(m: &ChatMessage) -> ProtocolMessageOut {
    ProtocolMessageOut {
        role: m.role.to_string(),
        content: m.content.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn try_from_system_instruction_trims_and_keeps() {
        let ctx = RequestContext {
            system_instruction: Some("  be brief  ".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert_eq!(domain.system_instruction.as_deref(), Some("be brief"));
    }

    #[test]
    fn try_from_blank_system_instruction_becomes_none() {
        let ctx = RequestContext {
            system_instruction: Some("  \n\t  ".into()),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert!(domain.system_instruction.is_none());
    }

    #[test]
    fn try_from_long_system_instruction_truncates() {
        let ctx = RequestContext {
            system_instruction: Some("x".repeat(SYSTEM_INSTRUCTION_MAX_BYTES + 1)),
            ..Default::default()
        };
        let domain = AgentTurnContext::try_from(ctx).expect("wire DTO conversion");
        assert_eq!(
            domain.system_instruction.as_ref().map(String::len),
            Some(SYSTEM_INSTRUCTION_MAX_BYTES)
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

    #[test]
    fn try_from_protocol_message_known_roles() {
        for role in ["user", "assistant", "tool", "system"] {
            let msg = ProtocolMessage {
                role: role.into(),
                content: "hi".into(),
            };
            let domain = ChatMessage::try_from(msg).expect("known role");
            assert_eq!(domain.role.as_wire(), role);
            assert_eq!(domain.content, "hi");
        }
    }

    #[test]
    fn try_from_protocol_message_unknown_role() {
        let msg = ProtocolMessage {
            role: "moderator".into(),
            content: "hi".into(),
        };
        let err = ChatMessage::try_from(msg).expect_err("unknown role");
        assert_eq!(
            err,
            ProtocolMessageConversionError::UnknownRole {
                role: "moderator".into()
            }
        );
    }
}
