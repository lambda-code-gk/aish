use crate::domain::human_task_checkpoint::HumanTaskWorkflowState;
use crate::ports::outbound::{HumanTaskStore, HumanTaskStoreError, HumanTaskTimeFormatter};

pub struct HumanTaskStatus<'a> {
    store: &'a dyn HumanTaskStore,
    time_formatter: &'a dyn HumanTaskTimeFormatter,
}
#[derive(Debug, thiserror::Error)]
pub enum HumanTaskStatusError {
    #[error("{0}")]
    Store(#[from] HumanTaskStoreError),
    #[error("human_task_checkpoint_invalid")]
    Invalid,
}
impl<'a> HumanTaskStatus<'a> {
    pub fn new(
        store: &'a dyn HumanTaskStore,
        time_formatter: &'a dyn HumanTaskTimeFormatter,
    ) -> Self {
        Self {
            store,
            time_formatter,
        }
    }
    pub fn render(&self) -> Result<String, HumanTaskStatusError> {
        match self.store.try_lock_exclusive()? {
            Some(_root_lock) => self.render_under_lock(),
            // Owner (create/resume) holds the lock for the whole Human Shell session.
            // Do not block; best-effort read of the stable Running checkpoint.
            None => self.render_while_owner_holds_lock(),
        }
    }

    fn render_under_lock(&self) -> Result<String, HumanTaskStatusError> {
        let checkpoint = match self.store.load_active() {
            Err(HumanTaskStoreError::NotFound) => return Ok("No suspended Human Task.\n".into()),
            other => other?,
        };
        if checkpoint.state == HumanTaskWorkflowState::Running {
            return Ok(format!(
                "Human Task: {}\nState: orphaned running\nObjective: {}\nCurrent cwd: {}\nRecovery:\n  ai human-task cancel --yes\n",
                checkpoint.task_id.as_str(),
                escape_status_field(&checkpoint.task.objective),
                escape_status_field(&checkpoint.current_cwd.to_string_lossy())
            ));
        }
        if checkpoint.state == HumanTaskWorkflowState::ResultPending {
            return Ok(format!(
                "Human Task: {}\nState: result pending\nObjective: {}\nCurrent cwd: {}\nContinue:\n  ai human-task resume\nCancel:\n  ai human-task cancel --yes\n",
                checkpoint.task_id.as_str(),
                escape_status_field(&checkpoint.task.objective),
                escape_status_field(&checkpoint.current_cwd.to_string_lossy())
            ));
        }
        if checkpoint.state == HumanTaskWorkflowState::Continuing {
            return Ok(format!(
                "Human Task: {}\nState: continuing\nObjective: {}\nCurrent cwd: {}\nA continuation turn was started. Automatic crash recovery is not available.\nCleanup:\n  ai human-task cancel --yes\n",
                checkpoint.task_id.as_str(),
                escape_status_field(&checkpoint.task.objective),
                escape_status_field(&checkpoint.current_cwd.to_string_lossy())
            ));
        }
        if checkpoint.state == HumanTaskWorkflowState::Finished {
            return Ok(format!(
                "Human Task: {}\nState: finished\nObjective: {}\nCurrent cwd: {}\nCleanup:\n  ai human-task cancel --yes\n",
                checkpoint.task_id.as_str(),
                escape_status_field(&checkpoint.task.objective),
                escape_status_field(&checkpoint.current_cwd.to_string_lossy())
            ));
        }
        if checkpoint.state != HumanTaskWorkflowState::Suspended {
            return Err(HumanTaskStatusError::Invalid);
        }
        let mut out = format!(
            "Human Task: {}\nState: suspended\nObjective: {}\nSuspended at: {}\nCurrent cwd: {}\n",
            checkpoint.task_id.as_str(),
            escape_status_field(&checkpoint.task.objective),
            self.time_formatter
                .format_local(checkpoint.suspended_at_ms.unwrap_or_default()),
            escape_status_field(&checkpoint.current_cwd.to_string_lossy())
        );
        if let Some(reason) = checkpoint.suspend_reason {
            out.push_str(&format!("Reason: {}\n", escape_status_field(&reason)));
        }
        out.push_str("Resume:\n  ai human-task resume\n");
        out.push_str("Cancel:\n  ai human-task cancel --yes\n");
        Ok(out)
    }

    fn render_while_owner_holds_lock(&self) -> Result<String, HumanTaskStatusError> {
        match self.store.load_active() {
            Err(HumanTaskStoreError::NotFound) => {
                Ok("Human Task is currently active in another ai process.\n".into())
            }
            Ok(checkpoint) if checkpoint.state == HumanTaskWorkflowState::Running => Ok(format!(
                "Human Task: {}\nState: running\nObjective: {}\nCurrent cwd: {}\nThis Human Shell session is active.\nSuspend:\n  human-task suspend [reason]\n",
                checkpoint.task_id.as_str(),
                escape_status_field(&checkpoint.task.objective),
                escape_status_field(&checkpoint.current_cwd.to_string_lossy())
            )),
            Ok(_) => Ok("Human Task is currently active in another ai process.\n".into()),
            Err(error) => Err(error.into()),
        }
    }
}

fn escape_status_field(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}
