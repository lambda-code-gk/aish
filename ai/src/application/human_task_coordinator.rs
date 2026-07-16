use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffFailure, HumanTaskBriefing, HumanTaskRequest,
    HumanTaskResult, ShellLogRange,
};

use crate::domain::human_task_checkpoint::{
    HumanShellSegment, HumanShellSegmentEnd, HumanTaskCheckpointV1, HumanTaskContinuationState,
    HumanTaskParentContext, HumanTaskWorkflowState,
};
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
    HumanTaskIdentity, HumanTaskStore, HumanTaskStoreError,
};

#[derive(Debug, Clone)]
pub struct HumanTaskParentInput {
    pub ai_session_id: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub user_request: String,
    pub cwd: PathBuf,
    pub llm_profile: String,
    pub runtime_dir: PathBuf,
}

pub struct HumanTaskCoordinator<'a> {
    store: &'a dyn HumanTaskStore,
    identity: &'a dyn HumanTaskIdentity,
    launcher: &'a dyn HumanShellLauncher,
    observer: &'a dyn EnvironmentObserver,
}

impl<'a> HumanTaskCoordinator<'a> {
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
        task: HumanTaskRequest,
        parent: HumanTaskParentInput,
        cancel: &AtomicBool,
    ) -> HumanTaskResult {
        let blocked = |code: &str| HumanTaskResult {
            status: HandoffExecutionOutcome::Blocked,
            task: task.clone(),
            verified: false,
            human_shell_exit_code: None,
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
            error: Some(HumanHandoffFailure {
                code: code.into(),
                message: code.into(),
            }),
            task_id: None,
            suspend_reason: None,
        };
        let _root_lock = match self.store.lock_exclusive() {
            Ok(lock) => lock,
            Err(_) => return blocked("human_task_checkpoint_unavailable"),
        };
        match self.store.load_active() {
            Ok(_) => return blocked("human_task_already_active"),
            Err(HumanTaskStoreError::NotFound) => {}
            Err(_) => return blocked("human_task_checkpoint_unavailable"),
        }
        let task_id = self.identity.new_task_id();
        let created = self.identity.now_ms();
        let mut checkpoint = HumanTaskCheckpointV1 {
            version: 1,
            task_id: task_id.clone(),
            state: HumanTaskWorkflowState::Running,
            task: task.clone(),
            parent: HumanTaskParentContext {
                ai_session_id: parent.ai_session_id,
                conversation_id: parent.conversation_id,
                turn_id: parent.turn_id,
                user_request: parent.user_request,
                original_cwd: parent.cwd.clone(),
                llm_profile: parent.llm_profile,
            },
            created_at_ms: created,
            updated_at_ms: created,
            suspended_at_ms: None,
            suspend_reason: None,
            current_cwd: parent.cwd.clone(),
            segments: Vec::new(),
            final_result: None,
            continuation: HumanTaskContinuationState::default(),
        };
        if self.store.save(&checkpoint).is_err() {
            return blocked("human_task_checkpoint_unavailable");
        }
        let request = HumanShellLaunchRequest {
            cwd: parent.cwd.clone(),
            parent_request_summary: String::new(),
            suggested_command: String::new(),
            runtime_dir: parent.runtime_dir,
            task_briefing: Some(HumanTaskBriefing::from(&task)),
        };
        let (returned, reason, suspended) = match self.launcher.launch_and_wait(&request, cancel) {
            Ok(returned) => (returned, None, false),
            Err(HumanShellLaunchError::Suspended { returned, reason }) => (*returned, reason, true),
            Err(HumanShellLaunchError::Cancelled(_)) => {
                if self.store.remove(&task_id).is_err() {
                    return blocked("human_task_checkpoint_unavailable");
                }
                return HumanTaskResult {
                    status: HandoffExecutionOutcome::Cancelled,
                    task,
                    verified: false,
                    human_shell_exit_code: None,
                    final_shell_cwd: None,
                    shell_log_range: None,
                    observation: None,
                    error: None,
                    task_id: None,
                    suspend_reason: None,
                };
            }
            Err(_) => {
                if self.store.remove(&task_id).is_err() {
                    return blocked("human_task_checkpoint_unavailable");
                }
                return blocked("human_task_launch_failed");
            }
        };
        let observation = self.observer.observe(
            &returned.final_cwd,
            returned.shell_log_start,
            Some(returned.shell_log_end),
            (!returned.shell_session_dir.as_os_str().is_empty())
                .then_some(returned.shell_session_dir.as_path()),
        );
        let range = ShellLogRange {
            start: returned.shell_log_start,
            end: Some(returned.shell_log_end),
        };
        if suspended {
            let ended = self.identity.now_ms();
            checkpoint.state = HumanTaskWorkflowState::Suspended;
            checkpoint.updated_at_ms = ended;
            checkpoint.suspended_at_ms = Some(ended);
            checkpoint.suspend_reason = reason.clone();
            checkpoint.current_cwd = returned.final_cwd.clone();
            checkpoint.segments.push(HumanShellSegment {
                index: 0,
                shell_session_id: returned.shell_session_id.clone(),
                started_at_ms: created,
                ended_at_ms: ended,
                initial_cwd: parent.cwd,
                final_cwd: returned.final_cwd.clone(),
                shell_log_range: range.clone(),
                observation: observation.clone(),
                end_reason: HumanShellSegmentEnd::Suspended,
            });
            if self.store.save(&checkpoint).is_err() {
                return blocked("human_task_checkpoint_unavailable");
            }
            HumanTaskResult {
                status: HandoffExecutionOutcome::Suspended,
                task,
                verified: false,
                human_shell_exit_code: returned.exit_code,
                final_shell_cwd: Some(returned.final_cwd.display().to_string()),
                shell_log_range: Some(range),
                observation: Some(observation),
                error: None,
                task_id: Some(task_id.as_str().into()),
                suspend_reason: reason,
            }
        } else {
            if self.store.remove(&task_id).is_err() {
                return blocked("human_task_checkpoint_unavailable");
            }
            HumanTaskResult {
                status: HandoffExecutionOutcome::Done,
                task,
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
    }
}
