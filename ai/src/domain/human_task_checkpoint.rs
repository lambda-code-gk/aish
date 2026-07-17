use std::path::PathBuf;

use aibe_protocol::{HumanTaskRequest, PostHandoffObservation, ShellLogRange};
use serde::{Deserialize, Serialize};

pub const HUMAN_TASK_CHECKPOINT_MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HumanTaskId(String);

impl HumanTaskId {
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        let bytes = value.as_bytes();
        let valid = bytes.len() == 18
            && bytes.starts_with(b"ht-")
            && bytes[3..11].iter().all(u8::is_ascii_digit)
            && bytes[11] == b'-'
            && bytes[12..]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        valid.then_some(Self(value)).ok_or("invalid human task id")
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for HumanTaskId {
    type Error = &'static str;
    fn try_from(v: String) -> Result<Self, Self::Error> {
        Self::parse(v)
    }
}
impl From<HumanTaskId> for String {
    fn from(v: HumanTaskId) -> Self {
        v.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanTaskWorkflowState {
    Running,
    Suspended,
    ResultPending,
    Continuing,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskParentContext {
    pub ai_session_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub user_request: String,
    pub original_cwd: PathBuf,
    pub llm_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanShellSegmentEnd {
    Suspended,
    Done,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanShellSegment {
    pub index: u32,
    pub shell_session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub initial_cwd: PathBuf,
    pub final_cwd: PathBuf,
    pub shell_log_range: ShellLogRange,
    pub observation: PostHandoffObservation,
    pub end_reason: HumanShellSegmentEnd,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskContinuationState {
    pub continuation_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanTaskCheckpointV1 {
    pub version: u8,
    pub task_id: HumanTaskId,
    pub state: HumanTaskWorkflowState,
    pub task: HumanTaskRequest,
    pub parent: HumanTaskParentContext,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub suspended_at_ms: Option<u64>,
    pub suspend_reason: Option<String>,
    pub current_cwd: PathBuf,
    pub segments: Vec<HumanShellSegment>,
    pub final_result: Option<aibe_protocol::HumanTaskResult>,
    pub continuation: HumanTaskContinuationState,
}

impl HumanTaskCheckpointV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != 1 {
            return Err("unsupported checkpoint version");
        }
        if self.created_at_ms > self.updated_at_ms
            || self.current_cwd.as_os_str().is_empty()
            || self.task.clone().normalized().is_err()
            || self.parent.ai_session_id.is_empty()
            || self.parent.conversation_id.is_empty()
            || self.parent.turn_id.is_empty()
            || self.parent.user_request.is_empty()
            || self.parent.original_cwd.as_os_str().is_empty()
            || self.parent.llm_profile.is_empty()
        {
            return Err("checkpoint invariant violated");
        }
        match self.state {
            HumanTaskWorkflowState::Running
                if self.suspended_at_ms.is_none()
                    && self.suspend_reason.is_none()
                    && self.final_result.is_none()
                    && self.continuation.continuation_turn_id.is_none()
                    && suspended_segments_are_contiguous(&self.segments) =>
            {
                Ok(())
            }
            HumanTaskWorkflowState::Suspended => {
                if let Some(reason) = &self.suspend_reason {
                    validate_suspend_reason(reason)?;
                }
                if self.suspended_at_ms.is_some()
                    && self.final_result.is_none()
                    && self.continuation.continuation_turn_id.is_none()
                    && (!self.segments.is_empty()
                        || self.suspend_reason.as_deref() == Some("unexpected_process_termination"))
                    && suspended_segments_are_contiguous(&self.segments)
                    && self
                        .segments
                        .last()
                        .is_none_or(|segment| self.current_cwd == segment.final_cwd)
                {
                    Ok(())
                } else {
                    Err("checkpoint invariant violated")
                }
            }
            HumanTaskWorkflowState::ResultPending
            | HumanTaskWorkflowState::Continuing
            | HumanTaskWorkflowState::Finished => {
                let Some(final_result) = &self.final_result else {
                    return Err("checkpoint invariant violated");
                };
                let turn_id_is_valid = match self.state {
                    HumanTaskWorkflowState::ResultPending => self
                        .continuation
                        .continuation_turn_id
                        .as_ref()
                        .is_none_or(|id| !id.is_empty()),
                    HumanTaskWorkflowState::Continuing | HumanTaskWorkflowState::Finished => self
                        .continuation
                        .continuation_turn_id
                        .as_ref()
                        .is_some_and(|id| !id.is_empty()),
                    _ => unreachable!(),
                };
                if self.suspended_at_ms.is_none()
                    && self.suspend_reason.is_none()
                    && turn_id_is_valid
                    && final_result.validate().is_ok()
                    && final_result.status == aibe_protocol::HandoffExecutionOutcome::Done
                    && result_pending_segments_are_valid(&self.segments)
                    && self.current_cwd == self.segments.last().unwrap().final_cwd
                {
                    Ok(())
                } else {
                    Err("checkpoint invariant violated")
                }
            }
            _ => Err("checkpoint invariant violated"),
        }
    }
}

fn suspended_segments_are_contiguous(segments: &[HumanShellSegment]) -> bool {
    segments.iter().enumerate().all(|(index, segment)| {
        segment.index == index as u32
            && segment.started_at_ms <= segment.ended_at_ms
            && segment.end_reason == HumanShellSegmentEnd::Suspended
    })
}

fn result_pending_segments_are_valid(segments: &[HumanShellSegment]) -> bool {
    let Some((last, prior)) = segments.split_last() else {
        return false;
    };
    suspended_segments_are_contiguous(prior)
        && last.index == prior.len() as u32
        && last.started_at_ms <= last.ended_at_ms
        && last.end_reason == HumanShellSegmentEnd::Done
}

pub fn validate_suspend_reason(reason: &str) -> Result<(), &'static str> {
    if reason.len() > 4096 || reason.chars().any(char::is_control) {
        Err("invalid suspend reason")
    } else {
        Ok(())
    }
}
