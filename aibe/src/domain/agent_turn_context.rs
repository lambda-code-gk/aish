//! エージェント turn 用クライアントコンテキスト（不変条件付き）。

use thiserror::Error;

use super::{ClientCwd, DelegationDepth, ShellLogTail, ToolName};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextError {
    #[error("context.cwd is required when tools are enabled")]
    MissingCwd,
}

/// 検証済みまたは組み立て済みの turn コンテキスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTurnContext {
    pub client_cwd: Option<ClientCwd>,
    pub shell_log_tail: Option<ShellLogTail>,
    /// クライアントがこの turn 用に渡した system 本文（注入のみ。aibe は解釈しない）。
    pub system_instruction: Option<String>,
    pub ai_session_id: Option<String>,
    /// クライアントが解決済みの memory space（注入時の解決順 1 位）。
    pub memory_space_id: Option<String>,
    pub collaborative_handoff: bool,
    pub execution_mode: aibe_protocol::ExecutionMode,
    pub delegation_depth: DelegationDepth,
}

impl AgentTurnContext {
    pub fn for_tool_turn(client_cwd: ClientCwd, tail: Option<ShellLogTail>) -> Self {
        Self {
            client_cwd: Some(client_cwd),
            shell_log_tail: tail,
            system_instruction: None,
            ai_session_id: None,
            memory_space_id: None,
            collaborative_handoff: false,
            execution_mode: aibe_protocol::ExecutionMode::Normal,
            delegation_depth: DelegationDepth::root(),
        }
    }

    pub fn for_text_only(tail: Option<ShellLogTail>) -> Self {
        Self {
            client_cwd: None,
            shell_log_tail: tail,
            system_instruction: None,
            ai_session_id: None,
            memory_space_id: None,
            collaborative_handoff: false,
            execution_mode: aibe_protocol::ExecutionMode::Normal,
            delegation_depth: DelegationDepth::root(),
        }
    }

    /// `tools` 非空時に cwd が揃っていることを検証。欠落時は `ContextError::MissingCwd`。
    pub fn validate_tools_enabled(&self, tools: &[ToolName]) -> Result<(), ContextError> {
        if tools.is_empty() {
            return Ok(());
        }
        if self.client_cwd.is_none() {
            return Err(ContextError::MissingCwd);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;

    #[test]
    fn validate_tools_enabled_empty_tools_ok_without_cwd() {
        let ctx = AgentTurnContext::for_text_only(None);
        assert!(ctx.validate_tools_enabled(&[]).is_ok());
    }

    #[test]
    fn validate_tools_enabled_requires_cwd_when_tools_non_empty() {
        let ctx = AgentTurnContext::for_text_only(None);
        assert_eq!(
            ctx.validate_tools_enabled(&[ToolName::read_file()]),
            Err(ContextError::MissingCwd)
        );
    }

    #[test]
    fn validate_tools_enabled_ok_with_cwd() {
        let cwd = ClientCwd::parse("/tmp/proj").expect("cwd");
        let ctx = AgentTurnContext::for_tool_turn(cwd, None);
        assert!(ctx.validate_tools_enabled(&[ToolName::read_file()]).is_ok());
    }
}
