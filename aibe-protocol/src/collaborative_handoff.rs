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
