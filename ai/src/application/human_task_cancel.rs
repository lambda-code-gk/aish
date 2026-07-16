use crate::domain::human_task_checkpoint::{HumanTaskCheckpointV1, HumanTaskWorkflowState};
use crate::ports::outbound::{HumanTaskStore, HumanTaskStoreError};

pub struct HumanTaskCancel<'a> {
    store: &'a dyn HumanTaskStore,
}

#[derive(Debug, thiserror::Error)]
pub enum HumanTaskCancelError {
    #[error("{0}")]
    Store(#[from] HumanTaskStoreError),
    #[error("human_task_checkpoint_invalid")]
    Invalid,
    #[error("human_task_cancel_not_confirmed")]
    NotConfirmed,
}

impl<'a> HumanTaskCancel<'a> {
    pub fn new(store: &'a dyn HumanTaskStore) -> Self {
        Self { store }
    }

    pub fn execute<F>(&self, confirm: F) -> Result<String, HumanTaskCancelError>
    where
        F: FnOnce(&HumanTaskCheckpointV1) -> bool,
    {
        let _root_lock = self.store.lock_exclusive()?;
        let checkpoint = match self.store.load_active() {
            Err(HumanTaskStoreError::NotFound) => return Ok("No suspended Human Task.\n".into()),
            other => other?,
        };
        if !matches!(
            checkpoint.state,
            HumanTaskWorkflowState::Suspended | HumanTaskWorkflowState::Running
        ) {
            return Err(HumanTaskCancelError::Invalid);
        }
        if !confirm(&checkpoint) {
            return Err(HumanTaskCancelError::NotConfirmed);
        }
        self.store.remove(&checkpoint.task_id)?;
        Ok(format!(
            "Cancelled Human Task {}.\n",
            checkpoint.task_id.as_str()
        ))
    }
}
