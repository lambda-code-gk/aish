//! 設定済み Agent Task Worker の effect boundary。

use std::path::PathBuf;

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{DelegationDepth, ValidatedAgentTaskRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskExecutionContext {
    pub cwd: PathBuf,
    pub delegation_depth: DelegationDepth,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerExecutionOutcome {
    Completed,
    Blocked,
    Cancelled,
    Failed,
    TimedOut,
    LaunchFailed,
    InvalidOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerExecutionOutput {
    pub outcome: WorkerExecutionOutcome,
    pub summary: String,
    pub reported_complete: bool,
    pub blockers: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub changed_paths: Vec<PathBuf>,
    pub observation_incomplete: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AgentTaskWorkerError {
    #[error("worker launch failed")]
    LaunchFailed,
    #[error("worker timed out")]
    TimedOut,
    #[error("worker output is invalid")]
    InvalidOutput,
    #[error("worker execution failed")]
    Failed,
}

#[async_trait]
pub trait AgentTaskWorker: Send + Sync {
    fn canonicalize_cwd(
        &self,
        candidate: &std::path::Path,
        allowed_roots: &[PathBuf],
    ) -> Result<PathBuf, AgentTaskWorkerError>;

    async fn execute(
        &self,
        request: ValidatedAgentTaskRequest,
        context: AgentTaskExecutionContext,
    ) -> Result<WorkerExecutionOutput, AgentTaskWorkerError>;
}
