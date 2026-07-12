//! Synchronous human handoff application service（0055 minimal / 0057 cancel）。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffResult, PostHandoffObservation,
    RequestedCommandCompletion, ShellLogRange,
};

use crate::domain::build_suggested_command;
use crate::ports::outbound::{
    EnvironmentObserver, HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher,
};

#[derive(Debug, Clone)]
pub struct HumanHandoffRequest {
    pub parent_request_summary: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub runtime_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanHandoffExecutionResult {
    pub execution_outcome: HandoffExecutionOutcome,
    pub requested_command: Option<String>,
    pub requested_command_completion: RequestedCommandCompletion,
    pub human_shell_exit_code: Option<i32>,
    pub final_shell_cwd: Option<String>,
    pub shell_log_range: Option<ShellLogRange>,
    pub observation: Option<PostHandoffObservation>,
}

#[derive(Debug, thiserror::Error)]
pub enum HumanHandoffError {
    #[error("collaborative handoff is not applicable")]
    NotApplicable,
    #[error("handoff cwd does not exist: {0}")]
    MissingCwd(String),
    #[error(transparent)]
    Launch(#[from] HumanShellLaunchError),
}

pub struct RunSynchronousHumanHandoff<'a> {
    shell_launcher: &'a dyn HumanShellLauncher,
    environment_observer: &'a dyn EnvironmentObserver,
}

impl<'a> RunSynchronousHumanHandoff<'a> {
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
        request: HumanHandoffRequest,
        cancel_requested: &AtomicBool,
    ) -> Result<HumanHandoffExecutionResult, HumanHandoffError> {
        if !request.cwd.is_dir() {
            return Err(HumanHandoffError::MissingCwd(
                request.cwd.display().to_string(),
            ));
        }
        let suggested_command = build_suggested_command(&request.command, &request.args);
        let shell_return = self.shell_launcher.launch_and_wait(
            &HumanShellLaunchRequest {
                cwd: request.cwd.clone(),
                parent_request_summary: request.parent_request_summary.clone(),
                suggested_command: suggested_command.clone(),
                runtime_dir: request.runtime_dir.clone(),
            },
            cancel_requested,
        )?;
        if !shell_return.normal_return {
            return Err(HumanHandoffError::Launch(
                HumanShellLaunchError::MissingReturnMarker,
            ));
        }
        let observation = self.environment_observer.observe(
            &shell_return.final_cwd,
            shell_return.shell_log_start,
            Some(shell_return.shell_log_end),
            if shell_return.shell_session_dir.as_os_str().is_empty() {
                None
            } else {
                Some(shell_return.shell_session_dir.as_path())
            },
        );
        Ok(HumanHandoffExecutionResult {
            execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
            requested_command: Some(suggested_command),
            requested_command_completion: RequestedCommandCompletion::Unknown,
            human_shell_exit_code: shell_return.exit_code,
            final_shell_cwd: Some(shell_return.final_cwd.display().to_string()),
            shell_log_range: Some(ShellLogRange {
                start: shell_return.shell_log_start,
                end: Some(shell_return.shell_log_end),
            }),
            observation: Some(observation),
        })
    }
}

pub fn handoff_tool_result_message() -> &'static str {
    "Control returned from the human shell.\n\nAISH did not automatically execute the requested command.\nThe shell exit code does not prove that the requested command ran or succeeded.\nInspect the current environment and verify the task state before continuing."
}

impl HumanHandoffExecutionResult {
    /// protocol DTO へ変換する。0060 では `collab_outcome` を付与しない。
    pub fn into_protocol_result(self) -> HumanHandoffResult {
        HumanHandoffResult {
            collab_outcome: None,
            execution_outcome: self.execution_outcome,
            requested_command: self.requested_command,
            requested_command_completion: self.requested_command_completion,
            human_shell_exit_code: self.human_shell_exit_code,
            final_shell_cwd: self.final_shell_cwd,
            shell_log_range: self.shell_log_range,
            observation: self.observation,
        }
    }
}
