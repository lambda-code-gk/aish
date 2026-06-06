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

/// `shell_exec` 監査用の承認結果（`with_shell_exec_audit`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecApprovalOutcome {
    /// `shell_exec_approval=never` によるポリシー拒否。
    PolicyNever,
    /// 承認フロー外（allowlist 不一致など）。
    NotApplicable,
    /// `always` による自動実行。
    AutoApproved,
    /// `ask` でユーザーが承認した後の実行（または実行失敗）。
    UserApproved,
    /// `ask` でユーザーが拒否。
    UserDenied,
    /// `ask` だが対話クライアント / gate が無い。
    ApprovalUnavailable,
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

    pub fn with_shell_exec_audit(
        mut self,
        approval_mode: &str,
        outcome: ShellExecApprovalOutcome,
        external_command: Option<&str>,
    ) -> Self {
        self.risk_class = Some(ToolRiskClass::DangerousShell);
        self.dry_run = Some(false);
        let mut approval_source = format!("shell_exec_approval={approval_mode}");
        if let Some(name) = external_command {
            approval_source.push_str(&format!(";external_command={name}"));
        }
        self.approval_source = Some(approval_source);
        match outcome {
            ShellExecApprovalOutcome::PolicyNever => {
                self.approval_state = Some(ToolApprovalState::NotRequired);
                self.decision = Some("rejected_by_policy".into());
            }
            ShellExecApprovalOutcome::NotApplicable => {
                self.approval_state = Some(ToolApprovalState::NotRequired);
                self.decision = Some(if self.status == ExecutedToolStatus::Ok {
                    "executed".into()
                } else {
                    "rejected_or_failed".into()
                });
            }
            ShellExecApprovalOutcome::AutoApproved => {
                self.approval_state = Some(ToolApprovalState::NotRequired);
                self.decision = Some(if self.status == ExecutedToolStatus::Ok {
                    "executed".into()
                } else {
                    "rejected_or_failed".into()
                });
            }
            ShellExecApprovalOutcome::UserApproved => {
                self.approval_state = Some(ToolApprovalState::ExplicitClientOptIn);
                self.decision = Some(if self.status == ExecutedToolStatus::Ok {
                    "executed".into()
                } else {
                    "rejected_or_failed".into()
                });
            }
            ShellExecApprovalOutcome::UserDenied => {
                self.approval_state = Some(ToolApprovalState::ExplicitClientOptIn);
                self.decision = Some("rejected_by_user".into());
            }
            ShellExecApprovalOutcome::ApprovalUnavailable => {
                self.approval_state = Some(ToolApprovalState::NotRequired);
                self.decision = Some("approval_unavailable".into());
            }
        }
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
    fn shell_exec_audit_unavailable_is_not_user_denied() {
        let tc = ExecutedToolCall::err(
            "c3".into(),
            ToolName::shell_exec().to_string(),
            serde_json::json!({"command": "echo"}),
            "approval_unavailable",
            "no gate",
        )
        .with_shell_exec_audit("ask", ShellExecApprovalOutcome::ApprovalUnavailable, None);
        assert_eq!(tc.approval_state, Some(ToolApprovalState::NotRequired));
        assert_eq!(tc.decision.as_deref(), Some("approval_unavailable"));
        assert_eq!(
            tc.approval_source.as_deref(),
            Some("shell_exec_approval=ask")
        );
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
