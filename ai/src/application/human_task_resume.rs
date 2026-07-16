use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use aibe_protocol::{HandoffExecutionOutcome, HumanTaskBriefing, HumanTaskResult, ShellLogRange};

use crate::domain::human_task_checkpoint::{
    HumanShellSegment, HumanShellSegmentEnd, HumanTaskCheckpointV1, HumanTaskId,
    HumanTaskWorkflowState,
};
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanShellReturn, HumanTaskIdentity, HumanTaskStore, HumanTaskStoreError,
};

pub struct HumanTaskResume<'a> {
    store: &'a dyn HumanTaskStore,
    identity: &'a dyn HumanTaskIdentity,
    launcher: &'a dyn HumanShellLauncher,
    observer: &'a dyn EnvironmentObserver,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HumanTaskResumeError {
    #[error("human_task_not_found")]
    NotFound,
    #[error("human_task_not_suspended")]
    NotSuspended,
    #[error("human_task_resume_cwd_unavailable")]
    CwdUnavailable,
    #[error("human_task_resume_launch_failed")]
    LaunchFailed,
    #[error("human_task_checkpoint_invalid")]
    Invalid,
    #[error("human_task_checkpoint_unavailable")]
    Unavailable,
}

impl From<HumanTaskStoreError> for HumanTaskResumeError {
    fn from(value: HumanTaskStoreError) -> Self {
        match value {
            HumanTaskStoreError::NotFound => Self::NotFound,
            HumanTaskStoreError::Invalid => Self::Invalid,
            HumanTaskStoreError::VersionUnsupported => Self::Invalid,
            HumanTaskStoreError::PermissionDenied => Self::Unavailable,
            HumanTaskStoreError::Unavailable => Self::Unavailable,
        }
    }
}

impl<'a> HumanTaskResume<'a> {
    pub fn new(
        store: &'a dyn HumanTaskStore,
        identity: &'a dyn HumanTaskIdentity,
        launcher: &'a dyn HumanShellLauncher,
        observer: &'a dyn EnvironmentObserver,
    ) -> Self {
        Self {
            store,
            identity,
            launcher,
            observer,
        }
    }

    pub fn execute(
        &self,
        task_id: Option<&str>,
        runtime_dir: PathBuf,
        cancel: &AtomicBool,
    ) -> Result<String, HumanTaskResumeError> {
        let _root_lock = self.store.lock_exclusive()?;
        let suspended = match self.store.load_active() {
            Err(HumanTaskStoreError::NotFound) => return Err(HumanTaskResumeError::NotFound),
            other => other?,
        };
        if let Some(requested) = task_id {
            let requested =
                HumanTaskId::parse(requested).map_err(|_| HumanTaskResumeError::NotFound)?;
            if requested != suspended.task_id {
                return Err(HumanTaskResumeError::NotFound);
            }
        }
        match suspended.state {
            HumanTaskWorkflowState::Suspended => {}
            HumanTaskWorkflowState::Running | HumanTaskWorkflowState::ResultPending => {
                return Err(HumanTaskResumeError::NotSuspended);
            }
            _ => return Err(HumanTaskResumeError::Invalid),
        }
        if !suspended.current_cwd.is_dir() {
            return Err(HumanTaskResumeError::CwdUnavailable);
        }

        let restore = suspended.clone();
        let started_at_ms = self.identity.now_ms();
        let initial_cwd = suspended.current_cwd.clone();
        let mut running = suspended;
        running.state = HumanTaskWorkflowState::Running;
        running.updated_at_ms = started_at_ms;
        running.suspended_at_ms = None;
        running.suspend_reason = None;
        self.store.save(&running)?;

        let request = HumanShellLaunchRequest {
            cwd: initial_cwd.clone(),
            parent_request_summary: String::new(),
            suggested_command: String::new(),
            runtime_dir,
            task_briefing: Some(HumanTaskBriefing::from(&running.task)),
        };
        let (returned, reason, suspended_outcome) = match self
            .launcher
            .launch_and_wait(&request, cancel)
        {
            Ok(returned) => (returned, None, false),
            Err(HumanShellLaunchError::Suspended { returned, reason }) => (*returned, reason, true),
            Err(HumanShellLaunchError::Cancelled(_)) | Err(_) => {
                self.restore_checkpoint(&restore)?;
                return Err(HumanTaskResumeError::LaunchFailed);
            }
        };

        let observation = self.observer.observe(
            &returned.final_cwd,
            returned.shell_log_start,
            Some(returned.shell_log_end),
            (!returned.shell_session_dir.as_os_str().is_empty())
                .then_some(returned.shell_session_dir.as_path()),
        );
        let ended = self.identity.now_ms();
        let next_index = running.segments.len() as u32;
        let range = ShellLogRange {
            start: returned.shell_log_start,
            end: Some(returned.shell_log_end),
        };
        let segment = HumanShellSegment {
            index: next_index,
            shell_session_id: returned.shell_session_id.clone(),
            started_at_ms,
            ended_at_ms: ended,
            initial_cwd,
            final_cwd: returned.final_cwd.clone(),
            shell_log_range: range.clone(),
            observation: observation.clone(),
            end_reason: if suspended_outcome {
                HumanShellSegmentEnd::Suspended
            } else {
                HumanShellSegmentEnd::Done
            },
        };
        running.updated_at_ms = ended;
        running.current_cwd = returned.final_cwd.clone();
        running.segments.push(segment);

        if suspended_outcome {
            running.state = HumanTaskWorkflowState::Suspended;
            running.suspended_at_ms = Some(ended);
            running.suspend_reason = reason;
            running.final_result = None;
            self.save_or_restore(&running, &restore)?;
            Ok(format!(
                "Human Task suspended.\n\nTask:\n  {}\n\nResume:\n  ai human-task resume\n\nCancel:\n  ai human-task cancel --yes\n",
                running.task_id.as_str()
            ))
        } else {
            let final_result = done_result(&running.task, &returned, range, observation);
            running.state = HumanTaskWorkflowState::ResultPending;
            running.suspended_at_ms = None;
            running.suspend_reason = None;
            running.final_result = Some(final_result);
            self.save_or_restore(&running, &restore)?;
            Ok(format!(
                "Human Task completed and saved.\n\nTask:\n  {}\nState: result pending\n\nAgent continuation is not available yet.\nCancel to discard:\n  ai human-task cancel --yes\n",
                running.task_id.as_str()
            ))
        }
    }

    fn save_or_restore(
        &self,
        checkpoint: &HumanTaskCheckpointV1,
        restore: &HumanTaskCheckpointV1,
    ) -> Result<(), HumanTaskResumeError> {
        if let Err(error) = self.store.save(checkpoint) {
            self.restore_checkpoint(restore)?;
            return Err(error.into());
        }
        Ok(())
    }

    fn restore_checkpoint(
        &self,
        restore: &HumanTaskCheckpointV1,
    ) -> Result<(), HumanTaskResumeError> {
        self.store.save(restore)?;
        Ok(())
    }
}

fn done_result(
    task: &aibe_protocol::HumanTaskRequest,
    returned: &HumanShellReturn,
    range: ShellLogRange,
    observation: aibe_protocol::PostHandoffObservation,
) -> HumanTaskResult {
    HumanTaskResult {
        status: HandoffExecutionOutcome::Done,
        task: task.clone(),
        verified: false,
        human_shell_exit_code: returned.exit_code,
        final_shell_cwd: Some(returned.final_cwd.display().to_string()),
        shell_log_range: Some(range),
        observation: Some(observation),
        error: None,
        task_id: None,
        suspend_reason: None,
    }
}
