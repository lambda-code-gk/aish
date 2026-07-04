//! Collaborative human handoff wire DTOs（0055）。

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
    Completed,
    NotExecuted,
}

/// shell log 参照範囲。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellLogRange {
    pub start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<u64>,
}

/// 復旧時に不確定なツール実行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UncertainToolExecution {
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: String,
}

/// 親 `shell_exec` handoff 完了時の synthetic tool result 本文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanHandoffResult {
    pub handoff_id: String,
    pub execution_outcome: HandoffExecutionOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_shell_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_command: Option<String>,
    pub requested_command_completion: RequestedCommandCompletion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_shell_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_range: Option<ShellLogRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_goal_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_conversation_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_observation_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_observation_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertain_tool_executions: Vec<UncertainToolExecution>,
}
