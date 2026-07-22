use std::sync::Mutex;

use async_trait::async_trait;

use crate::domain::ValidatedAgentTaskRequest;
use crate::ports::outbound::{
    AgentTaskExecutionContext, AgentTaskWorker, AgentTaskWorkerError, WorkerExecutionOutput,
};

pub struct MockWorker {
    outcome: Mutex<Result<WorkerExecutionOutput, AgentTaskWorkerError>>,
    calls: Mutex<Vec<(ValidatedAgentTaskRequest, AgentTaskExecutionContext)>>,
}

impl MockWorker {
    pub fn new(outcome: Result<WorkerExecutionOutput, AgentTaskWorkerError>) -> Self {
        Self {
            outcome: Mutex::new(outcome),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<(ValidatedAgentTaskRequest, AgentTaskExecutionContext)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl AgentTaskWorker for MockWorker {
    fn canonicalize_cwd(
        &self,
        candidate: &std::path::Path,
        allowed_roots: &[std::path::PathBuf],
    ) -> Result<std::path::PathBuf, AgentTaskWorkerError> {
        let canonical = candidate
            .canonicalize()
            .map_err(|_| AgentTaskWorkerError::Failed)?;
        if !canonical.is_dir()
            || !allowed_roots.iter().any(|root| {
                root.canonicalize()
                    .is_ok_and(|root| canonical.starts_with(root))
            })
        {
            return Err(AgentTaskWorkerError::Failed);
        }
        Ok(canonical)
    }

    async fn execute(
        &self,
        request: ValidatedAgentTaskRequest,
        context: AgentTaskExecutionContext,
    ) -> Result<WorkerExecutionOutput, AgentTaskWorkerError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((request, context));
        }
        self.outcome
            .lock()
            .map(|outcome| outcome.clone())
            .unwrap_or(Err(AgentTaskWorkerError::Failed))
    }
}
