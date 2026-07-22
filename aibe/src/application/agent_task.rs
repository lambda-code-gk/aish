use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::domain::{
    AgentTaskEvidence, AgentTaskEvidenceKind, AgentTaskEvidenceSource, AgentTaskRequest,
    AgentTaskResult, AgentTaskStatus, DelegationDepth,
};
use crate::ports::outbound::{
    AgentTaskApprovalOutcome, AgentTaskApprovalPrompt, AgentTaskExecutionContext,
    AgentTaskWorkerRegistry, ToolExecutionContext, WorkerExecutionOutcome,
};

pub const AGENT_TASK_TRUST_WARNING: &str = "The worker's internal operations are not individually approved by AISH; cwd is a launch directory, not an OS sandbox.";

pub struct AgentTaskService {
    registry: Arc<dyn AgentTaskWorkerRegistry>,
    enabled: bool,
    allowed_roots: Vec<PathBuf>,
    max_output_bytes: usize,
    server_timeout_secs: u64,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AgentTaskServiceError {
    #[error("agent_task is disabled")]
    Disabled,
    #[error("delegated contexts cannot call agent_task")]
    RecursiveDelegation,
    #[error("unknown agent_task worker")]
    UnknownWorker,
    #[error("invalid agent_task request: {0}")]
    InvalidRequest(String),
    #[error("invalid or unauthorized cwd")]
    InvalidCwd,
    #[error("agent_task approval unavailable")]
    ApprovalUnavailable,
    #[error("agent_task approval denied")]
    ApprovalDenied,
    #[error("agent_task approval cancelled")]
    ApprovalCancelled,
    #[error("agent_task approval timed out")]
    ApprovalTimeout,
}

impl AgentTaskService {
    pub fn new(
        registry: Arc<dyn AgentTaskWorkerRegistry>,
        enabled: bool,
        allowed_roots: Vec<PathBuf>,
        max_output_bytes: usize,
        server_timeout_secs: u64,
    ) -> Self {
        Self {
            registry,
            enabled,
            allowed_roots,
            max_output_bytes,
            server_timeout_secs,
        }
    }

    pub fn published_for(&self, depth: DelegationDepth) -> bool {
        self.enabled && !self.registry.is_empty() && depth.permits_delegation()
    }

    pub async fn execute(
        &self,
        tool_call_id: &str,
        request: AgentTaskRequest,
        ctx: &ToolExecutionContext,
    ) -> Result<AgentTaskResult, AgentTaskServiceError> {
        if !self.enabled {
            return Err(AgentTaskServiceError::Disabled);
        }
        if !ctx.delegation_depth().permits_delegation() {
            return Err(AgentTaskServiceError::RecursiveDelegation);
        }
        let request = request
            .validate_shape()
            .map_err(|e| AgentTaskServiceError::InvalidRequest(e.to_string()))?;
        let worker_timeout = self
            .registry
            .timeout_limit_secs(&request.worker)
            .ok_or(AgentTaskServiceError::UnknownWorker)?;
        let permission_profile = self
            .registry
            .permission_profile(&request.worker)
            .ok_or(AgentTaskServiceError::UnknownWorker)?
            .to_string();
        let worker = self
            .registry
            .get(&request.worker)
            .ok_or(AgentTaskServiceError::UnknownWorker)?;
        let validated = request
            .validate(worker_timeout, self.server_timeout_secs)
            .map_err(|e| AgentTaskServiceError::InvalidRequest(e.to_string()))?;
        let cwd = self.resolve_cwd(validated.cwd.as_deref(), ctx, worker.as_ref())?;
        let prompt = AgentTaskApprovalPrompt {
            worker: validated.worker.as_str().to_string(),
            cwd: cwd.to_string_lossy().into_owned(),
            timeout_secs: validated.timeout_secs,
            permission_profile,
            objective: validated.objective.clone(),
            trust_boundary_warning: AGENT_TASK_TRUST_WARNING.into(),
        };
        let approval = if let Some(gate) = ctx.agent_task_approval_gate() {
            gate.request_agent_task_approval(tool_call_id, prompt.clone())
                .await
        } else if let Some(gate) = ctx.tool_approval_gate() {
            use crate::ports::outbound::{ToolApprovalGateOutcome, ToolApprovalPromptRequest};
            match gate
                .request_tool_approval(
                    tool_call_id,
                    ToolApprovalPromptRequest {
                        tool_name: crate::domain::AGENT_TASK.into(),
                        summary: format!(
                            "worker={} timeout={}s profile={} objective={}",
                            prompt.worker,
                            prompt.timeout_secs,
                            prompt.permission_profile,
                            prompt.objective
                        ),
                        paths: vec![prompt.cwd.clone()],
                        preview: prompt.trust_boundary_warning.clone(),
                        preview_truncated: false,
                    },
                )
                .await
            {
                ToolApprovalGateOutcome::Approved(aibe_protocol::ToolApprovalOrigin::UiYes) => {
                    AgentTaskApprovalOutcome::Approved {
                        origin: "explicit_ui".into(),
                    }
                }
                ToolApprovalGateOutcome::Approved(origin) => AgentTaskApprovalOutcome::Denied {
                    origin: format!("non_explicit:{origin:?}"),
                },
                ToolApprovalGateOutcome::Denied(origin) => AgentTaskApprovalOutcome::Denied {
                    origin: format!("{origin:?}"),
                },
                ToolApprovalGateOutcome::Unavailable => AgentTaskApprovalOutcome::Unavailable,
                ToolApprovalGateOutcome::Cancelled => AgentTaskApprovalOutcome::Cancelled,
                ToolApprovalGateOutcome::Timeout => AgentTaskApprovalOutcome::Timeout,
            }
        } else {
            AgentTaskApprovalOutcome::Unavailable
        };
        let approval_origin = match approval {
            AgentTaskApprovalOutcome::Approved { origin } if origin == "explicit_ui" => origin,
            AgentTaskApprovalOutcome::Approved { .. } => {
                return Err(AgentTaskServiceError::ApprovalDenied)
            }
            AgentTaskApprovalOutcome::Denied { .. } => {
                return Err(AgentTaskServiceError::ApprovalDenied)
            }
            AgentTaskApprovalOutcome::Unavailable => {
                return Err(AgentTaskServiceError::ApprovalUnavailable)
            }
            AgentTaskApprovalOutcome::Cancelled => {
                return Err(AgentTaskServiceError::ApprovalCancelled)
            }
            AgentTaskApprovalOutcome::Timeout => {
                return Err(AgentTaskServiceError::ApprovalTimeout)
            }
        };
        let output = worker
            .execute(
                validated.clone(),
                AgentTaskExecutionContext {
                    cwd: cwd.clone(),
                    delegation_depth: DelegationDepth::delegated(),
                    max_output_bytes: self.max_output_bytes,
                },
            )
            .await;
        Ok(normalize_result(
            output,
            approval_origin,
            validated.worker.as_str(),
            &cwd,
            validated.timeout_secs,
        ))
    }

    fn resolve_cwd(
        &self,
        requested: Option<&str>,
        ctx: &ToolExecutionContext,
        worker: &dyn crate::ports::outbound::AgentTaskWorker,
    ) -> Result<PathBuf, AgentTaskServiceError> {
        let candidate = requested
            .map(Path::new)
            .map(|path| ctx.resolve_path(path))
            .unwrap_or_else(|| ctx.base_dir().to_path_buf());
        let roots = if self.allowed_roots.is_empty() {
            vec![ctx.base_dir().to_path_buf()]
        } else {
            self.allowed_roots
                .iter()
                .map(|root| ctx.resolve_path(root))
                .collect()
        };
        worker
            .canonicalize_cwd(&candidate, &roots)
            .map_err(|_| AgentTaskServiceError::InvalidCwd)
    }
}

fn normalize_result(
    output: Result<
        crate::ports::outbound::WorkerExecutionOutput,
        crate::ports::outbound::AgentTaskWorkerError,
    >,
    approval_origin: String,
    worker: &str,
    cwd: &Path,
    timeout_secs: u64,
) -> AgentTaskResult {
    let cwd = cwd.to_string_lossy().into_owned();
    let mut evidence = Vec::new();
    match output {
        Ok(output) => {
            for path in &output.changed_paths {
                evidence.push(AgentTaskEvidence::unverified(
                    AgentTaskEvidenceKind::WorkspaceChange,
                    AgentTaskEvidenceSource::WorkspaceObserver,
                    format!("changed path: {}", path.display()),
                ));
            }
            let status = match output.outcome {
                WorkerExecutionOutcome::Completed => AgentTaskStatus::Completed,
                WorkerExecutionOutcome::Failed => AgentTaskStatus::Failed,
                WorkerExecutionOutcome::TimedOut => AgentTaskStatus::TimedOut,
                WorkerExecutionOutcome::LaunchFailed => AgentTaskStatus::LaunchFailed,
                WorkerExecutionOutcome::InvalidOutput => AgentTaskStatus::InvalidOutput,
            };
            evidence.push(AgentTaskEvidence::unverified(
                AgentTaskEvidenceKind::WorkerReport,
                AgentTaskEvidenceSource::WorkerProcess,
                output.summary.clone(),
            ));
            evidence.push(AgentTaskEvidence::unverified(
                AgentTaskEvidenceKind::ExitStatus,
                AgentTaskEvidenceSource::WorkerProcess,
                format!("exit code: {:?}", output.exit_code),
            ));
            if !output.stdout.is_empty() || !output.stderr.is_empty() {
                evidence.push(AgentTaskEvidence::unverified(
                    AgentTaskEvidenceKind::ProcessOutput,
                    AgentTaskEvidenceSource::WorkerProcess,
                    "bounded worker stdout/stderr captured",
                ));
            }
            AgentTaskResult::unverified(
                status,
                output.summary,
                output.reported_complete,
                output.stdout,
                output.stderr,
                output.stdout_truncated,
                output.stderr_truncated,
                output.exit_code,
                matches!(output.outcome, WorkerExecutionOutcome::TimedOut),
                output.changed_paths,
                output.observation_incomplete,
                evidence,
                approval_origin,
                worker,
                cwd,
                timeout_secs,
            )
        }
        Err(error) => {
            let status = match error {
                crate::ports::outbound::AgentTaskWorkerError::LaunchFailed => {
                    AgentTaskStatus::LaunchFailed
                }
                crate::ports::outbound::AgentTaskWorkerError::TimedOut => AgentTaskStatus::TimedOut,
                crate::ports::outbound::AgentTaskWorkerError::InvalidOutput => {
                    AgentTaskStatus::InvalidOutput
                }
                crate::ports::outbound::AgentTaskWorkerError::Failed => AgentTaskStatus::Failed,
            };
            AgentTaskResult::unverified(
                status,
                error.to_string(),
                false,
                String::new(),
                String::new(),
                false,
                false,
                None,
                status == AgentTaskStatus::TimedOut,
                Vec::new(),
                true,
                evidence,
                approval_origin,
                worker,
                cwd,
                timeout_secs,
            )
        }
    }
}
