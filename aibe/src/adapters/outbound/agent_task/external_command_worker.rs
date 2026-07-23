use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::adapters::outbound::tools::subprocess::{run_subprocess_bounded, BoundedRunOutcome};
use crate::domain::{
    AgentTaskCriterion, ValidatedAgentTaskRequest, MAX_BLOCKERS, MAX_BLOCKER_BYTES,
};
use crate::ports::outbound::{
    AgentTaskExecutionContext, AgentTaskWorker, AgentTaskWorkerConfig, AgentTaskWorkerError,
    WorkerExecutionOutcome, WorkerExecutionOutput,
};

use super::{observe_changes, snapshot_workspace};

pub struct ExternalCommandWorker {
    config: AgentTaskWorkerConfig,
}

impl ExternalCommandWorker {
    pub fn new(config: AgentTaskWorkerConfig) -> Self {
        Self { config }
    }
}

#[derive(Serialize)]
struct WorkerEnvelope<'a> {
    schema_version: u8,
    objective: &'a str,
    instructions: &'a [String],
    completion_criteria: &'a [AgentTaskCriterion],
    cwd: String,
    delegation_depth: u8,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkerReportStatus {
    Done,
    Blocked,
    Cancelled,
    Failed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerReport {
    schema_version: u8,
    summary: String,
    status: WorkerReportStatus,
    #[serde(default)]
    blockers: Vec<String>,
}

#[async_trait]
impl AgentTaskWorker for ExternalCommandWorker {
    fn canonicalize_cwd(
        &self,
        candidate: &std::path::Path,
        allowed_roots: &[std::path::PathBuf],
    ) -> Result<std::path::PathBuf, AgentTaskWorkerError> {
        let canonical = candidate
            .canonicalize()
            .map_err(|_| AgentTaskWorkerError::Failed)?;
        if !canonical.is_dir() {
            return Err(AgentTaskWorkerError::Failed);
        }
        let allowed = allowed_roots.iter().any(|root| {
            root.canonicalize()
                .is_ok_and(|root| canonical.starts_with(root))
        });
        allowed
            .then_some(canonical)
            .ok_or(AgentTaskWorkerError::Failed)
    }

    async fn execute(
        &self,
        request: ValidatedAgentTaskRequest,
        context: AgentTaskExecutionContext,
    ) -> Result<WorkerExecutionOutput, AgentTaskWorkerError> {
        let envelope = WorkerEnvelope {
            schema_version: 1,
            objective: &request.objective,
            instructions: &request.instructions,
            completion_criteria: &request.completion_criteria,
            cwd: context.cwd.to_string_lossy().into_owned(),
            delegation_depth: context.delegation_depth.get(),
        };
        let stdin = serde_json::to_vec(&envelope).map_err(|_| AgentTaskWorkerError::Failed)?;
        let before = snapshot_workspace(&context.cwd);
        let mut command = Command::new(&self.config.executable);
        command
            .args(&self.config.args)
            .current_dir(&context.cwd)
            .env_clear()
            .env("AISH_DELEGATION_DEPTH", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in &self.config.env_allowlist {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let run = run_subprocess_bounded(
            command,
            stdin,
            Duration::from_secs(request.timeout_secs),
            context.max_output_bytes,
        )
        .await;
        let after = snapshot_workspace(&context.cwd);
        let (changed_paths, observation_incomplete) = observe_changes(&before, &after);
        match run {
            BoundedRunOutcome::Failed => Ok(WorkerExecutionOutput {
                outcome: WorkerExecutionOutcome::LaunchFailed,
                summary: "worker launch failed".into(),
                reported_complete: false,
                blockers: Vec::new(),
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                exit_code: None,
                changed_paths,
                observation_incomplete,
            }),
            BoundedRunOutcome::TimedOut => Ok(WorkerExecutionOutput {
                outcome: WorkerExecutionOutcome::TimedOut,
                summary: "worker timed out".into(),
                reported_complete: false,
                blockers: Vec::new(),
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                exit_code: None,
                changed_paths,
                observation_incomplete,
            }),
            BoundedRunOutcome::Completed {
                exit_code,
                stdout,
                stderr,
                stdout_truncated,
                stderr_truncated,
            } => {
                let stdout_text = String::from_utf8_lossy(&stdout).into_owned();
                let stderr_text = String::from_utf8_lossy(&stderr).into_owned();
                if exit_code != 0 {
                    return Ok(WorkerExecutionOutput {
                        outcome: WorkerExecutionOutcome::Failed,
                        summary: "worker returned non-zero exit status".into(),
                        reported_complete: false,
                        blockers: Vec::new(),
                        stdout: stdout_text,
                        stderr: stderr_text,
                        stdout_truncated,
                        stderr_truncated,
                        exit_code: Some(exit_code),
                        changed_paths,
                        observation_incomplete,
                    });
                }
                let report: WorkerReport = match serde_json::from_slice::<WorkerReport>(&stdout) {
                    Ok(report) if report.schema_version == 1 => report,
                    _ => {
                        return Ok(WorkerExecutionOutput {
                            outcome: WorkerExecutionOutcome::InvalidOutput,
                            summary: "worker returned invalid structured output".into(),
                            reported_complete: false,
                            blockers: Vec::new(),
                            stdout: stdout_text,
                            stderr: stderr_text,
                            stdout_truncated,
                            stderr_truncated,
                            exit_code: Some(exit_code),
                            changed_paths,
                            observation_incomplete,
                        });
                    }
                };
                let blockers = bound_blockers(report.blockers);
                let (outcome, reported_complete) = match report.status {
                    WorkerReportStatus::Done => (WorkerExecutionOutcome::Completed, true),
                    WorkerReportStatus::Blocked => (WorkerExecutionOutcome::Blocked, false),
                    WorkerReportStatus::Cancelled => (WorkerExecutionOutcome::Cancelled, false),
                    WorkerReportStatus::Failed => (WorkerExecutionOutcome::Failed, false),
                };
                if matches!(report.status, WorkerReportStatus::Blocked) && blockers.is_empty() {
                    return Ok(WorkerExecutionOutput {
                        outcome: WorkerExecutionOutcome::InvalidOutput,
                        summary: "blocked worker report requires at least one blocker".into(),
                        reported_complete: false,
                        blockers: Vec::new(),
                        stdout: stdout_text,
                        stderr: stderr_text,
                        stdout_truncated,
                        stderr_truncated,
                        exit_code: Some(exit_code),
                        changed_paths,
                        observation_incomplete,
                    });
                }
                Ok(WorkerExecutionOutput {
                    outcome,
                    summary: report.summary,
                    reported_complete,
                    blockers,
                    stdout: stdout_text,
                    stderr: stderr_text,
                    stdout_truncated,
                    stderr_truncated,
                    exit_code: Some(exit_code),
                    changed_paths,
                    observation_incomplete,
                })
            }
        }
    }
}

fn bound_blockers(mut blockers: Vec<String>) -> Vec<String> {
    blockers.truncate(MAX_BLOCKERS);
    blockers
        .into_iter()
        .map(|item| {
            let mut value = item;
            if value.len() > MAX_BLOCKER_BYTES {
                value.truncate(value.floor_char_boundary(MAX_BLOCKER_BYTES));
            }
            value
        })
        .filter(|item| !item.trim().is_empty())
        .collect()
}
