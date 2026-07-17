use std::path::PathBuf;

use aibe_protocol::{AgentTurnStatus, ClientResponse};

use crate::domain::human_task_checkpoint::{
    HumanTaskCheckpointV1, HumanTaskId, HumanTaskWorkflowState,
};
use crate::ports::outbound::{HumanTaskStore, HumanTaskStoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskContinuationRequest {
    pub turn_id: String,
    pub message: String,
    pub ai_session_id: String,
    pub conversation_id: String,
    pub cwd: PathBuf,
    pub llm_profile: String,
}

pub struct HumanTaskContinuation<'a> {
    store: &'a dyn HumanTaskStore,
    now_ms: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HumanTaskContinuationError {
    #[error("human_task_not_found")]
    NotFound,
    #[error("human_task_continuation_not_pending")]
    NotPending,
    #[error("human_task_continuation_cwd_unavailable")]
    CwdUnavailable,
    #[error("human_task_continuation_failed")]
    TurnFailed,
    #[error("human_task_checkpoint_invalid")]
    Invalid,
    #[error("human_task_checkpoint_unavailable")]
    Unavailable,
}

/// Continuation turn success is only `AgentTurnStatus::Ok`.
/// `MaxToolRounds` shares `AgentTurnResult` but must not finish/delete the checkpoint.
pub fn continuation_turn_succeeded(response: &ClientResponse) -> bool {
    matches!(
        response,
        ClientResponse::AgentTurnResult {
            status: AgentTurnStatus::Ok,
            ..
        }
    )
}

impl From<HumanTaskStoreError> for HumanTaskContinuationError {
    fn from(value: HumanTaskStoreError) -> Self {
        match value {
            HumanTaskStoreError::NotFound => Self::NotFound,
            HumanTaskStoreError::Invalid | HumanTaskStoreError::VersionUnsupported => Self::Invalid,
            HumanTaskStoreError::PermissionDenied | HumanTaskStoreError::Unavailable => {
                Self::Unavailable
            }
        }
    }
}

impl<'a> HumanTaskContinuation<'a> {
    pub fn new(store: &'a dyn HumanTaskStore, now_ms: u64) -> Self {
        Self { store, now_ms }
    }

    pub fn execute<F>(
        &self,
        task_id: Option<&str>,
        run_turn: F,
    ) -> Result<String, HumanTaskContinuationError>
    where
        F: FnOnce(&HumanTaskContinuationRequest) -> Result<(), ()>,
    {
        let _root_lock = self.store.lock_exclusive()?;
        let mut checkpoint = self.store.load_active()?;
        ensure_task_id(task_id, &checkpoint.task_id)?;
        if checkpoint.state != HumanTaskWorkflowState::ResultPending {
            return Err(HumanTaskContinuationError::NotPending);
        }
        if !checkpoint.current_cwd.is_dir() {
            return Err(HumanTaskContinuationError::CwdUnavailable);
        }

        if checkpoint.continuation.continuation_turn_id.is_none() {
            checkpoint.continuation.continuation_turn_id =
                Some(format!("{}-continuation", checkpoint.task_id.as_str()));
            self.store.save(&checkpoint)?;
        }
        let request = continuation_request(&checkpoint)?;
        checkpoint.state = HumanTaskWorkflowState::Continuing;
        checkpoint.updated_at_ms = self.now_ms.max(checkpoint.updated_at_ms);
        self.store.save(&checkpoint)?;

        if run_turn(&request).is_err() {
            checkpoint.state = HumanTaskWorkflowState::ResultPending;
            checkpoint.updated_at_ms = self.now_ms.max(checkpoint.updated_at_ms);
            self.store.save(&checkpoint)?;
            return Err(HumanTaskContinuationError::TurnFailed);
        }

        checkpoint.state = HumanTaskWorkflowState::Finished;
        checkpoint.updated_at_ms = self.now_ms.max(checkpoint.updated_at_ms);
        self.store.save(&checkpoint)?;
        self.store.remove(&checkpoint.task_id)?;
        Ok(format!(
            "Human Task continuation finished.\n\nTask:\n  {}\n",
            checkpoint.task_id.as_str()
        ))
    }
}

fn ensure_task_id(
    requested: Option<&str>,
    actual: &HumanTaskId,
) -> Result<(), HumanTaskContinuationError> {
    let Some(requested) = requested else {
        return Ok(());
    };
    let requested =
        HumanTaskId::parse(requested).map_err(|_| HumanTaskContinuationError::NotFound)?;
    if &requested == actual {
        Ok(())
    } else {
        Err(HumanTaskContinuationError::NotFound)
    }
}

fn continuation_request(
    checkpoint: &HumanTaskCheckpointV1,
) -> Result<HumanTaskContinuationRequest, HumanTaskContinuationError> {
    let turn_id = checkpoint
        .continuation
        .continuation_turn_id
        .clone()
        .ok_or(HumanTaskContinuationError::Invalid)?;
    Ok(HumanTaskContinuationRequest {
        turn_id,
        message: build_human_task_continuation_message(checkpoint)?,
        ai_session_id: checkpoint.parent.ai_session_id.clone(),
        conversation_id: checkpoint.parent.conversation_id.clone(),
        cwd: checkpoint.current_cwd.clone(),
        llm_profile: checkpoint.parent.llm_profile.clone(),
    })
}

pub fn build_human_task_continuation_message(
    checkpoint: &HumanTaskCheckpointV1,
) -> Result<String, HumanTaskContinuationError> {
    let result = checkpoint
        .final_result
        .as_ref()
        .ok_or(HumanTaskContinuationError::Invalid)?;
    let task = serde_json::to_string_pretty(&checkpoint.task)
        .map_err(|_| HumanTaskContinuationError::Invalid)?;
    let result =
        serde_json::to_string_pretty(result).map_err(|_| HumanTaskContinuationError::Invalid)?;
    Ok(format!(
        "[Collaborative Mode continuation]\n\nA previous agent turn delegated a Human Task and then stopped.\n\nOriginal user request:\n{}\n\nHuman Task:\n{}\n\nHuman Task result:\n{}\n\nImportant:\n- The Human Task result is unverified.\n- Re-observe the environment where possible.\n- Verify the completion criteria before claiming completion.\n- Continue the original user request from this point.\n",
        checkpoint.parent.user_request, task, result
    ))
}
