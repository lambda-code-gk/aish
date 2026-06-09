//! `agent_turn` 用 `RequestContext` の組み立て。

use aibe_protocol::{RequestContext, SYSTEM_INSTRUCTION_MAX_BYTES};

#[derive(Debug, Clone, Default)]
pub struct RequestContextInput {
    pub shell_log_tail: Option<String>,
    pub cwd: Option<String>,
    pub ai_session_id: Option<String>,
    pub conversation_id: Option<String>,
    /// この turn のみ LLM に前置する system 本文（会話履歴には載せない）。
    pub system_instruction: Option<String>,
}

impl RequestContextInput {
    pub fn into_wire(self) -> RequestContext {
        RequestContext {
            shell_log_tail: self.shell_log_tail,
            cwd: self.cwd,
            ai_session_id: self.ai_session_id,
            conversation_id: self.conversation_id,
            system_instruction: normalize_system_instruction(self.system_instruction),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_wire_omits_system_instruction_when_none() {
        let ctx = RequestContextInput::default().into_wire();
        assert!(ctx.system_instruction.is_none());
    }

    #[test]
    fn into_wire_includes_system_instruction() {
        let ctx = RequestContextInput {
            system_instruction: Some("be brief".into()),
            ..Default::default()
        }
        .into_wire();
        assert_eq!(ctx.system_instruction.as_deref(), Some("be brief"));
    }

    #[test]
    fn into_wire_truncates_system_instruction() {
        let raw = "x".repeat(SYSTEM_INSTRUCTION_MAX_BYTES + 1);
        let ctx = RequestContextInput {
            system_instruction: Some(raw),
            ..Default::default()
        }
        .into_wire();
        assert_eq!(
            ctx.system_instruction.as_ref().map(String::len),
            Some(SYSTEM_INSTRUCTION_MAX_BYTES)
        );
    }
}
