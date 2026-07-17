use crate::domain::human_task_checkpoint::{HumanTaskCheckpointV1, HumanTaskWorkflowState};
use crate::ports::outbound::{HumanTaskStore, HumanTaskStoreError};

pub struct HumanTaskRecover<'a> {
    store: &'a dyn HumanTaskStore,
    now_ms: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HumanTaskRecoverError {
    #[error("human_task_checkpoint_busy")]
    Busy,
    #[error("human_task_recovery_not_confirmed")]
    NotConfirmed,
    #[error("human_task_recovery_force_invalid_required")]
    ForceInvalidRequired,
    #[error(
        "human_task_already_recoverable; use ai human-task resume or ai human-task cancel --yes"
    )]
    AlreadyRecoverable,
    #[error("human_task_finished; use ai human-task cancel --yes")]
    Finished,
    #[error("{0}")]
    Store(#[from] HumanTaskStoreError),
}

impl<'a> HumanTaskRecover<'a> {
    pub fn new(store: &'a dyn HumanTaskStore, now_ms: u64) -> Self {
        Self { store, now_ms }
    }

    pub fn execute<F>(
        &self,
        force_invalid: bool,
        confirm: F,
    ) -> Result<String, HumanTaskRecoverError>
    where
        F: FnOnce(Option<&HumanTaskCheckpointV1>) -> bool,
    {
        let Some(_root_lock) = self.store.try_lock_exclusive()? else {
            return Err(HumanTaskRecoverError::Busy);
        };
        let mut checkpoint = match self.store.load_active() {
            Ok(checkpoint) => checkpoint,
            Err(HumanTaskStoreError::Invalid)
            | Err(HumanTaskStoreError::VersionUnsupported)
            | Err(HumanTaskStoreError::PermissionDenied) => {
                if !force_invalid {
                    return Err(HumanTaskRecoverError::ForceInvalidRequired);
                }
                if !confirm(None) {
                    return Err(HumanTaskRecoverError::NotConfirmed);
                }
                let residue = self.store.remove_invalid_active()?;
                return Ok(format!("Removed invalid Human Task residue {residue}.\n"));
            }
            Err(error) => return Err(error.into()),
        };
        if force_invalid {
            return Err(HumanTaskRecoverError::AlreadyRecoverable);
        }
        match checkpoint.state {
            HumanTaskWorkflowState::Running | HumanTaskWorkflowState::Continuing => {}
            HumanTaskWorkflowState::Finished => return Err(HumanTaskRecoverError::Finished),
            HumanTaskWorkflowState::Suspended | HumanTaskWorkflowState::ResultPending => {
                return Err(HumanTaskRecoverError::AlreadyRecoverable);
            }
        }
        if !confirm(Some(&checkpoint)) {
            return Err(HumanTaskRecoverError::NotConfirmed);
        }
        checkpoint.updated_at_ms = checkpoint.updated_at_ms.max(self.now_ms);
        let output = match checkpoint.state {
            HumanTaskWorkflowState::Running => {
                checkpoint.state = HumanTaskWorkflowState::Suspended;
                checkpoint.suspended_at_ms = Some(checkpoint.updated_at_ms);
                checkpoint.suspend_reason = Some("unexpected_process_termination".into());
                format!(
                    "Recovered Human Task {} as suspended.\nResume:\n  ai human-task resume\n",
                    checkpoint.task_id.as_str()
                )
            }
            HumanTaskWorkflowState::Continuing => {
                checkpoint.state = HumanTaskWorkflowState::ResultPending;
                format!(
                    "Recovered Human Task {} as result pending.\nRetry continuation:\n  ai human-task resume\n",
                    checkpoint.task_id.as_str()
                )
            }
            _ => unreachable!(),
        };
        self.store.save(&checkpoint)?;
        Ok(output)
    }
}
