//! クライアント向け `tool_calls` 記録（wire）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 実行済みツール呼び出しの成否。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutedToolStatus {
    Ok,
    Error,
}

/// ツールの危険度クラス（監査用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskClass {
    ReadOnly,
    DangerousShell,
    WriteLike,
}

/// 承認状態（監査用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalState {
    NotRequired,
    ExplicitClientOptIn,
}

/// クライアント向け `tool_calls` 記録。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub status: ExecutedToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_class: Option<ToolRiskClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<ToolApprovalState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_source: Option<String>,
}

impl ExecutedToolCall {
    pub fn ok(id: String, name: impl Into<String>, arguments: Value, output: String) -> Self {
        Self {
            id,
            name: name.into(),
            arguments,
            status: ExecutedToolStatus::Ok,
            output: Some(output),
            error: None,
            message: None,
            risk_class: None,
            approval_state: None,
            dry_run: None,
            decision: None,
            approval_source: None,
        }
    }

    pub fn err(
        id: String,
        name: impl Into<String>,
        arguments: Value,
        error: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            arguments,
            status: ExecutedToolStatus::Error,
            output: None,
            error: Some(error.into()),
            message: Some(message.into()),
            risk_class: None,
            approval_state: None,
            dry_run: None,
            decision: None,
            approval_source: None,
        }
    }

    pub fn with_audit(
        mut self,
        risk_class: ToolRiskClass,
        approval_state: ToolApprovalState,
        dry_run: bool,
    ) -> Self {
        self.risk_class = Some(risk_class);
        self.approval_state = Some(approval_state);
        self.dry_run = Some(dry_run);
        self.decision = Some(
            match self.status {
                ExecutedToolStatus::Ok => "executed",
                ExecutedToolStatus::Error => "rejected_or_failed",
            }
            .to_string(),
        );
        self.approval_source = Some(
            match approval_state {
                ToolApprovalState::NotRequired => "none",
                ToolApprovalState::ExplicitClientOptIn => "client_tools_allowlist",
            }
            .to_string(),
        );
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolName;

    #[test]
    fn executed_tool_call_serde_roundtrip() {
        let tc = ExecutedToolCall::ok(
            "c1".into(),
            ToolName::shell_exec().to_string(),
            serde_json::json!({"command": "echo"}),
            "hi".into(),
        );
        let json = serde_json::to_string(&tc).expect("serialize");
        assert!(json.contains(r#""name":"shell_exec""#));
        let back: ExecutedToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.name, tc.name);
        assert_eq!(back.output, tc.output);
    }

    #[test]
    fn executed_tool_call_with_audit_roundtrip() {
        let tc = ExecutedToolCall::ok(
            "c2".into(),
            ToolName::read_file().to_string(),
            serde_json::json!({"path": "README.md"}),
            "ok".into(),
        )
        .with_audit(
            ToolRiskClass::ReadOnly,
            ToolApprovalState::NotRequired,
            false,
        );
        let json = serde_json::to_string(&tc).expect("serialize");
        let back: ExecutedToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.risk_class, Some(ToolRiskClass::ReadOnly));
        assert_eq!(back.approval_state, Some(ToolApprovalState::NotRequired));
        assert_eq!(back.dry_run, Some(false));
        assert_eq!(back.decision.as_deref(), Some("executed"));
        assert_eq!(back.approval_source.as_deref(), Some("none"));
    }
}
