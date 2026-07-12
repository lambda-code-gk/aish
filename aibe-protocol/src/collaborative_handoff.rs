//! Minimal human handoff wire DTOs（0055）。

use serde::{Deserialize, Serialize};

/// human shell から制御が戻ったときの実行結果カテゴリ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffExecutionOutcome {
    HumanControlReturned,
}

/// 親が要求したコマンドの完了可否（human shell 返却時は不明が既定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedCommandCompletion {
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollabOutcomeStatus {
    Done,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollabOutcome {
    pub status: CollabOutcomeStatus,
}

/// shell log 参照範囲。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellLogRange {
    pub start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

/// human handoff 失敗の structured error（user denial と区別する）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffFailure {
    pub code: String,
    pub message: String,
}

/// human shell 終了後の軽量再観測。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostHandoffObservation {
    pub cwd_exists: bool,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observation_errors: Vec<String>,
}

/// 親 `shell_exec` handoff 完了時の synthetic tool result 本文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffResult {
    /// 0060 以降、成功 handoff では省略する（0059 必須契約の撤回）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collab_outcome: Option<CollabOutcome>,
    pub execution_outcome: HandoffExecutionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_command: Option<String>,
    pub requested_command_completion: RequestedCommandCompletion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_shell_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_shell_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_range: Option<ShellLogRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation: Option<PostHandoffObservation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_task_briefing_adds_no_protocol_schema() {
        let without = HumanHandoffResult {
            collab_outcome: None,
            execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
            requested_command: None,
            requested_command_completion: RequestedCommandCompletion::Unknown,
            human_shell_exit_code: Some(0),
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
        };
        let json = serde_json::to_value(&without).unwrap();
        assert!(json.get("collab_outcome").is_none());

        let missing = r#"{"execution_outcome":"human_control_returned","requested_command_completion":"unknown"}"#;
        let decoded: HumanHandoffResult = serde_json::from_str(missing).unwrap();
        assert!(decoded.collab_outcome.is_none());

        let legacy = r#"{"collab_outcome":{"status":"done"},"execution_outcome":"human_control_returned","requested_command_completion":"unknown"}"#;
        let decoded_legacy: HumanHandoffResult = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            decoded_legacy.collab_outcome,
            Some(CollabOutcome {
                status: CollabOutcomeStatus::Done
            })
        );
    }
}
