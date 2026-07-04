//! Collaborative human handoff domain（0055 Phase 1）。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::domain::shell_single_quote;

pub const HANDOFF_SCHEMA_VERSION: u32 = 1;

/// 協調作業におけるエージェント役割。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborativeAgentRole {
    Parent,
    Side,
    Standalone,
}

/// 協調 policy（親のみ handoff を起動）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollaborativePolicy {
    Enabled,
    Disabled,
}

/// Handoff 状態（spec §9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HandoffState {
    Creating,
    HumanActive,
    SideAgentRunning,
    SideAgentWaitingForHuman,
    Returned,
    Orphaned,
    ResumingParent,
    Completed,
    Cancelled,
}

/// 状態遷移イベント。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffEvent {
    ShellReady,
    ShellLaunchFailed,
    Cancel,
    StartSideAgent,
    SideAgentWaiting,
    SideAgentResumed,
    SideAgentReturned,
    HumanReturned,
    Orphaned,
    Resume,
    StartParentResume,
    ParentResumeCompleted,
    ParentResumeFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid handoff transition from {state:?} on {event:?}")]
pub struct HandoffTransitionError {
    pub state: HandoffState,
    pub event: HandoffEvent,
}

/// 親 `shell_exec` 要求の保存形。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedShellExec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// command candidate の出所。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandCandidateSource {
    ParentAgent,
    SideAgent,
    History,
    Manual,
}

/// プロンプト挿入候補。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandCandidate {
    pub id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: CommandCandidateSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub target_handoff_id: String,
    pub created_at_ms: u64,
}

/// human shell セッション世代。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffShellSession {
    pub generation: u32,
    pub token_hash: String,
    pub created_at_ms: u64,
}

/// child goal 終了理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildGoalCloseReason {
    ControlReturned,
}

/// child goal 達成可否。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildGoalAchievement {
    Unknown,
    Achieved,
    NotAchieved,
}

/// child goal メタ（memory `goal` 連携用 ID のみ保持）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildGoalMeta {
    pub id: String,
    pub handoff_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_goal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<ChildGoalCloseReason>,
    pub achievement: ChildGoalAchievement,
}

/// Handoff 本体（主要フィールド、spec §24.1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    pub id: String,
    pub schema_version: u32,
    pub parent_task_id: String,
    pub parent_conversation_id: String,
    pub parent_run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_goal_id: Option<String>,
    pub child_goal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_conversation_id: Option<String>,
    pub state: HandoffState,
    pub initial_cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_shell_cwd: Option<String>,
    pub parent_request_summary: String,
    #[serde(default)]
    pub requested_shell_execs: Vec<RequestedShellExec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_human_request: Option<String>,
    pub conversation_snapshot_ref: String,
    pub conversation_summary: String,
    pub checkpoint_ref: String,
    pub before_observation_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_observation_ref: Option<String>,
    pub shell_log_start: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_log_end: Option<u64>,
    pub shell_generation: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_shell_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Handoff lease（spec §24.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffLease {
    pub handoff_id: String,
    pub owner_client_id: String,
    pub owner_process_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_tty: Option<String>,
    pub owner_host: String,
    pub owner_uid: u32,
    pub lease_acquired_at_ms: u64,
    pub lease_expires_at_ms: u64,
    pub last_heartbeat_at_ms: u64,
}

/// 復旧 checkpoint（spec §24.3 必須フィールド）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffCheckpoint {
    pub parent_task_id: String,
    pub parent_conversation_id: String,
    pub parent_run_id: String,
    pub pending_shell_exec: RequestedShellExec,
    pub parent_goal: String,
    pub child_goal: ChildGoalMeta,
    pub conversation_snapshot: String,
    pub conversation_summary: String,
    pub cwd: String,
    pub environment_metadata: String,
    pub handoff_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_conversation_id: Option<String>,
    #[serde(default)]
    pub command_candidates: Vec<CommandCandidate>,
    pub shell_log_start: u64,
    pub control_state: HandoffState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<String>,
    /// handoff 中断時に確定できなかった tool execution。
    #[serde(default)]
    pub tool_executions: Vec<RecoverableToolExecution>,
}

/// handoff checkpoint に保存する provider 非依存の tool execution 状態。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverableToolExecution {
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: RecoverableToolStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoverableToolStatus {
    Requested,
    Running,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

/// process 消失後は RUNNING の成否を推測せず UNKNOWN に確定する。
pub fn mark_running_tools_unknown(checkpoint: &mut HandoffCheckpoint) -> Vec<String> {
    checkpoint
        .tool_executions
        .iter_mut()
        .filter_map(|tool| {
            if tool.status == RecoverableToolStatus::Running {
                tool.status = RecoverableToolStatus::Unknown;
                Some(tool.tool_call_id.clone())
            } else {
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid handoff id")]
pub struct InvalidHandoffIdError;

/// path component として安全な handoff ID か検証する。
pub fn is_valid_handoff_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.chars().all(|c| c == '.')
        && !id.contains('/')
        && !id.contains('\\')
        && !id.contains("..")
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn validate_handoff_id(id: &str) -> Result<(), InvalidHandoffIdError> {
    if is_valid_handoff_id(id) {
        Ok(())
    } else {
        Err(InvalidHandoffIdError)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("cancel handoff failed: {0}")]
pub struct CancelHandoffError(#[from] HandoffTransitionError);

/// checkpoint に含める必須フィールド名（検証用）。
pub const CHECKPOINT_REQUIRED_FIELD_NAMES: &[&str] = &[
    "parent_task_id",
    "parent_conversation_id",
    "parent_run_id",
    "pending_shell_exec",
    "parent_goal",
    "child_goal",
    "conversation_snapshot",
    "conversation_summary",
    "cwd",
    "environment_metadata",
    "handoff_id",
    "command_candidates",
    "shell_log_start",
    "control_state",
];

pub fn try_transition(
    state: HandoffState,
    event: HandoffEvent,
) -> Result<HandoffState, HandoffTransitionError> {
    let next = match (state, event) {
        (HandoffState::Creating, HandoffEvent::ShellReady) => HandoffState::HumanActive,
        (HandoffState::Creating, HandoffEvent::ShellLaunchFailed)
        | (HandoffState::Creating, HandoffEvent::Cancel) => HandoffState::Cancelled,
        // launcher adapter は side process が接続できるよう spawn 直前に HUMAN_ACTIVE を
        // durable にする。exec 自体が失敗した場合だけこの補償遷移を使う。
        (HandoffState::HumanActive, HandoffEvent::ShellLaunchFailed) => HandoffState::Cancelled,
        (HandoffState::HumanActive, HandoffEvent::StartSideAgent) => HandoffState::SideAgentRunning,
        (HandoffState::HumanActive, HandoffEvent::HumanReturned) => HandoffState::Returned,
        (HandoffState::SideAgentRunning, HandoffEvent::SideAgentWaiting) => {
            HandoffState::SideAgentWaitingForHuman
        }
        (HandoffState::SideAgentWaitingForHuman, HandoffEvent::SideAgentResumed) => {
            HandoffState::SideAgentRunning
        }
        (HandoffState::SideAgentRunning, HandoffEvent::SideAgentReturned) => {
            HandoffState::HumanActive
        }
        (
            HandoffState::SideAgentRunning | HandoffState::SideAgentWaitingForHuman,
            HandoffEvent::HumanReturned,
        ) => HandoffState::Returned,
        (
            HandoffState::Creating
            | HandoffState::HumanActive
            | HandoffState::SideAgentRunning
            | HandoffState::SideAgentWaitingForHuman,
            HandoffEvent::Orphaned,
        ) => HandoffState::Orphaned,
        (HandoffState::Orphaned, HandoffEvent::Resume) => HandoffState::HumanActive,
        (HandoffState::Returned, HandoffEvent::StartParentResume) => HandoffState::ResumingParent,
        (HandoffState::ResumingParent, HandoffEvent::ParentResumeCompleted) => {
            HandoffState::Completed
        }
        (HandoffState::ResumingParent, HandoffEvent::ParentResumeFailed) => HandoffState::Returned,
        _ => {
            return Err(HandoffTransitionError { state, event });
        }
    };
    Ok(next)
}

/// shell 起動前失敗などで handoff を取り消す。
pub fn cancel_handoff(handoff: &mut Handoff) -> Result<(), CancelHandoffError> {
    handoff.state = try_transition(handoff.state, HandoffEvent::Cancel)?;
    Ok(())
}

/// `command` + `args[]` から候補文字列を組み立てる（分解しない）。
pub fn build_candidate_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        return command.to_string();
    }
    let mut out = command.to_string();
    for arg in args {
        out.push(' ');
        out.push_str(&shell_single_quote(arg));
    }
    out
}

pub fn hash_handoff_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(token.as_bytes());
    hex_encode(&digest)
}

pub fn verify_handoff_token(token: &str, token_hash: &str) -> bool {
    hash_handoff_token(token) == token_hash
}

/// 最新 generation の token のみ有効。旧 generation は失効扱い。
pub fn validate_shell_token(
    sessions: &[HandoffShellSession],
    token: &str,
    generation: u32,
) -> bool {
    let current_max = sessions.iter().map(|s| s.generation).max().unwrap_or(0);
    if generation != current_max {
        return false;
    }
    sessions
        .iter()
        .rev()
        .find(|s| s.generation == current_max)
        .is_some_and(|s| verify_handoff_token(token, &s.token_hash))
}

/// human shell 正常返却時のみ child goal を閉じる。
pub fn close_child_goal_on_control_returned(goal: &mut ChildGoalMeta) {
    goal.close_reason = Some(ChildGoalCloseReason::ControlReturned);
    goal.achievement = ChildGoalAchievement::Unknown;
}

pub fn should_close_child_goal(state: HandoffState) -> bool {
    matches!(state, HandoffState::Returned)
}

pub fn checkpoint_has_required_fields(checkpoint: &HandoffCheckpoint) -> bool {
    !checkpoint.parent_task_id.is_empty()
        && !checkpoint.parent_conversation_id.is_empty()
        && !checkpoint.parent_run_id.is_empty()
        && !checkpoint.pending_shell_exec.command.is_empty()
        && !checkpoint.parent_goal.is_empty()
        && !checkpoint.child_goal.id.is_empty()
        && !checkpoint.conversation_snapshot.is_empty()
        && !checkpoint.conversation_summary.is_empty()
        && !checkpoint.cwd.is_empty()
        && !checkpoint.environment_metadata.is_empty()
        && !checkpoint.handoff_id.is_empty()
}

pub fn checkpoint_serialized_field_names() -> HashSet<String> {
    let sample = HandoffCheckpoint {
        parent_task_id: String::new(),
        parent_conversation_id: String::new(),
        parent_run_id: String::new(),
        pending_shell_exec: RequestedShellExec {
            command: String::new(),
            args: Vec::new(),
            cwd: None,
            tool_call_id: None,
        },
        parent_goal: String::new(),
        child_goal: ChildGoalMeta {
            id: String::new(),
            handoff_id: String::new(),
            parent_goal_id: None,
            close_reason: None,
            achievement: ChildGoalAchievement::Unknown,
        },
        conversation_snapshot: String::new(),
        conversation_summary: String::new(),
        cwd: String::new(),
        environment_metadata: String::new(),
        handoff_id: String::new(),
        side_conversation_id: None,
        command_candidates: Vec::new(),
        shell_log_start: 0,
        control_state: HandoffState::Creating,
        provider_metadata: None,
        tool_executions: Vec::new(),
    };
    let value = serde_json::to_value(sample).expect("checkpoint sample");
    value
        .as_object()
        .expect("checkpoint object")
        .keys()
        .cloned()
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn transition_matrix_covers_normal_and_side_paths() {
        assert_eq!(
            try_transition(HandoffState::Creating, HandoffEvent::ShellReady).unwrap(),
            HandoffState::HumanActive
        );
        assert_eq!(
            try_transition(HandoffState::HumanActive, HandoffEvent::StartSideAgent).unwrap(),
            HandoffState::SideAgentRunning
        );
        assert_eq!(
            try_transition(
                HandoffState::SideAgentRunning,
                HandoffEvent::SideAgentWaiting
            )
            .unwrap(),
            HandoffState::SideAgentWaitingForHuman
        );
        assert_eq!(
            try_transition(
                HandoffState::SideAgentRunning,
                HandoffEvent::SideAgentReturned
            )
            .unwrap(),
            HandoffState::HumanActive
        );
        assert_eq!(
            try_transition(HandoffState::Orphaned, HandoffEvent::Resume).unwrap(),
            HandoffState::HumanActive
        );
        assert!(try_transition(HandoffState::Orphaned, HandoffEvent::ShellReady).is_err());
        assert!(try_transition(HandoffState::Completed, HandoffEvent::Resume).is_err());
        assert!(
            try_transition(HandoffState::HumanActive, HandoffEvent::SideAgentReturned).is_err()
        );
    }

    #[test]
    fn handoff_id_rejects_path_traversal() {
        assert!(is_valid_handoff_id("ho-1"));
        assert!(!is_valid_handoff_id("../escape"));
        assert!(!is_valid_handoff_id("foo/bar"));
        assert!(!is_valid_handoff_id(""));
    }
}
