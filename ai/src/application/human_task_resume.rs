use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use aibe_protocol::{HumanTaskBriefing, ShellLogRange};

use crate::domain::human_task_checkpoint::{
    HumanShellSegment, HumanShellSegmentEnd, HumanTaskCheckpointV1, HumanTaskId,
    HumanTaskWorkflowState,
};
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanTaskIdentity, HumanTaskStore, HumanTaskStoreError,
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
    #[error("human_task_resume_completion_deferred")]
    CompletionDeferred,
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
            HumanTaskWorkflowState::Running => return Err(HumanTaskResumeError::NotSuspended),
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
                self.restore_suspended(&restore)?;
                return Err(HumanTaskResumeError::LaunchFailed);
            }
        };

        if !suspended_outcome {
            self.restore_suspended(&restore)?;
            return Err(HumanTaskResumeError::CompletionDeferred);
        }

        let observation = self.observer.observe(
            &returned.final_cwd,
            returned.shell_log_start,
            Some(returned.shell_log_end),
            (!returned.shell_session_dir.as_os_str().is_empty())
                .then_some(returned.shell_session_dir.as_path()),
        );
        let ended = self.identity.now_ms();
        let next_index = running.segments.len() as u32;
        running.state = HumanTaskWorkflowState::Suspended;
        running.updated_at_ms = ended;
        running.suspended_at_ms = Some(ended);
        running.suspend_reason = reason;
        running.current_cwd = returned.final_cwd.clone();
        running.segments.push(HumanShellSegment {
            index: next_index,
            shell_session_id: returned.shell_session_id,
            started_at_ms,
            ended_at_ms: ended,
            initial_cwd,
            final_cwd: returned.final_cwd,
            shell_log_range: ShellLogRange {
                start: returned.shell_log_start,
                end: Some(returned.shell_log_end),
            },
            observation,
            end_reason: HumanShellSegmentEnd::Suspended,
        });
        self.store.save(&running)?;
        Ok(format!(
            "Human Task suspended.\n\nTask:\n  {}\n\nResume:\n  ai human-task resume\n\nCancel:\n  ai human-task cancel --yes\n",
            running.task_id.as_str()
        ))
    }

    fn restore_suspended(
        &self,
        restore: &HumanTaskCheckpointV1,
    ) -> Result<(), HumanTaskResumeError> {
        self.store.save(restore)?;
        Ok(())
    }
}
