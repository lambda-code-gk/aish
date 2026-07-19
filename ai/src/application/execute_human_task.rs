use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffFailure, HumanTaskBriefing, HumanTaskRequest,
    HumanTaskResult, ShellLogRange,
};

use crate::domain::suggested_command_from_instructions;
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
};

pub struct ExecuteHumanTask<'a> {
    shell_launcher: &'a dyn HumanShellLauncher,
    environment_observer: &'a dyn EnvironmentObserver,
}

impl<'a> ExecuteHumanTask<'a> {
    pub fn new(
        shell_launcher: &'a dyn HumanShellLauncher,
        environment_observer: &'a dyn EnvironmentObserver,
    ) -> Self {
        Self {
            shell_launcher,
            environment_observer,
        }
    }

    pub fn execute(
        &self,
        task: HumanTaskRequest,
        cwd: PathBuf,
        runtime_dir: PathBuf,
        cancel: &AtomicBool,
    ) -> HumanTaskResult {
        let blocked = |code: &str, message: String| HumanTaskResult {
            status: HandoffExecutionOutcome::Blocked,
            task: task.clone(),
            verified: false,
            human_shell_exit_code: None,
            final_shell_cwd: None,
            shell_log_range: None,
            observation: None,
            error: Some(HumanHandoffFailure {
                code: code.into(),
                message,
            }),
            task_id: None,
            suspend_reason: None,
        };
        if !cwd.is_dir() {
            return blocked(
                "human_task_cwd_unavailable",
                "human task cwd is unavailable".into(),
            );
        }
        let launch = HumanShellLaunchRequest {
            cwd,
            parent_request_summary: String::new(),
            suggested_command: suggested_command_from_instructions(&task.instructions),
            runtime_dir,
            task_briefing: Some(HumanTaskBriefing::from(&task)),
        };
        let returned = match self.shell_launcher.launch_and_wait(&launch, cancel) {
            Ok(value) if value.outcome == crate::ports::outbound::HumanShellOutcome::Done => value,
            Ok(_) => {
                return blocked(
                    "human_task_missing_return_marker",
                    "human shell ended without a normal return marker".into(),
                )
            }
            Err(HumanShellLaunchError::Cancelled(_)) => {
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
                }
            }
            Err(_) => {
                return blocked(
                    "human_task_launch_failed",
                    "human shell lifecycle could not be established".into(),
                )
            }
        };
        let observation = self.environment_observer.observe(
            &returned.final_cwd,
            returned.shell_log_start,
            Some(returned.shell_log_end),
            (!returned.shell_session_dir.as_os_str().is_empty())
                .then_some(returned.shell_session_dir.as_path()),
        );
        HumanTaskResult {
            status: HandoffExecutionOutcome::Done,
            task,
            verified: false,
            human_shell_exit_code: returned.exit_code,
            final_shell_cwd: Some(returned.final_cwd.display().to_string()),
            shell_log_range: Some(ShellLogRange {
                start: returned.shell_log_start,
                end: Some(returned.shell_log_end),
            }),
            observation: Some(observation),
            error: None,
            task_id: None,
            suspend_reason: None,
        }
    }
}
