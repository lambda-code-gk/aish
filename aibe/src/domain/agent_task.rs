//! Agent Task Delegation の純粋な request/result 契約。

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_OBJECTIVE_BYTES: usize = 4096;
pub const MAX_INSTRUCTION_BYTES: usize = 2048;
pub const MAX_INSTRUCTIONS: usize = 32;
pub const MAX_INSTRUCTIONS_BYTES: usize = 16 * 1024;
pub const MAX_CRITERIA: usize = 32;
pub const MAX_CRITERION_ID_BYTES: usize = 64;
pub const MAX_CRITERION_DESCRIPTION_BYTES: usize = 2048;
pub const MAX_CRITERIA_BYTES: usize = 32 * 1024;
pub const MAX_CWD_BYTES: usize = 4096;
pub const MAX_TIMEOUT_SECS: u64 = 1800;
pub const MAX_EVIDENCE: usize = 256;
pub const MAX_EVIDENCE_SUMMARY_BYTES: usize = 1024;
pub const MAX_BLOCKERS: usize = 32;
pub const MAX_BLOCKER_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkerId(String);

impl WorkerId {
    pub fn parse(value: impl Into<String>) -> Result<Self, AgentTaskValidationError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value.as_bytes()[0]
                .is_ascii_lowercase()
                .then_some(())
                .is_some()
            || (!value.is_empty() && value.len() <= 64 && value.as_bytes()[0].is_ascii_digit());
        if !valid
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(AgentTaskValidationError::InvalidWorkerId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for WorkerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTaskRequest {
    pub worker: WorkerId,
    pub objective: String,
    pub instructions: Vec<String>,
    pub completion_criteria: Vec<AgentTaskCriterion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentTaskCriterion {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DelegationDepth(u8);

impl DelegationDepth {
    pub fn root() -> Self {
        Self(0)
    }

    pub fn delegated() -> Self {
        Self(1)
    }

    pub fn get(self) -> u8 {
        self.0
    }

    pub fn permits_delegation(self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAgentTaskRequest {
    pub worker: WorkerId,
    pub objective: String,
    pub instructions: Vec<String>,
    pub completion_criteria: Vec<AgentTaskCriterion>,
    pub cwd: Option<String>,
    pub timeout_secs: u64,
}

impl AgentTaskRequest {
    /// Registry lookup 前に schema / 件数 / byte 上限だけを検査する。
    pub fn validate_shape(self) -> Result<Self, AgentTaskValidationError> {
        bounded_non_empty("objective", &self.objective, MAX_OBJECTIVE_BYTES)?;
        if self.instructions.is_empty() || self.instructions.len() > MAX_INSTRUCTIONS {
            return Err(AgentTaskValidationError::InvalidInstructions);
        }
        let mut instruction_bytes = 0usize;
        for item in &self.instructions {
            bounded_non_empty("instruction", item, MAX_INSTRUCTION_BYTES)?;
            instruction_bytes = instruction_bytes.saturating_add(item.len());
        }
        if instruction_bytes > MAX_INSTRUCTIONS_BYTES {
            return Err(AgentTaskValidationError::InvalidInstructions);
        }
        if self.completion_criteria.is_empty() || self.completion_criteria.len() > MAX_CRITERIA {
            return Err(AgentTaskValidationError::InvalidCriteria);
        }
        let mut ids = BTreeSet::new();
        let mut criteria_bytes = 0usize;
        for criterion in &self.completion_criteria {
            bounded_non_empty("criterion id", &criterion.id, MAX_CRITERION_ID_BYTES)?;
            bounded_non_empty(
                "criterion description",
                &criterion.description,
                MAX_CRITERION_DESCRIPTION_BYTES,
            )?;
            if !ids.insert(&criterion.id) {
                return Err(AgentTaskValidationError::DuplicateCriterionId(
                    criterion.id.clone(),
                ));
            }
            criteria_bytes = criteria_bytes
                .saturating_add(criterion.id.len())
                .saturating_add(criterion.description.len());
        }
        if criteria_bytes > MAX_CRITERIA_BYTES {
            return Err(AgentTaskValidationError::InvalidCriteria);
        }
        if let Some(cwd) = &self.cwd {
            if cwd.is_empty() || cwd.len() > MAX_CWD_BYTES || cwd.as_bytes().contains(&0) {
                return Err(AgentTaskValidationError::InvalidCwd);
            }
        }
        if let Some(timeout_secs) = self.timeout_secs {
            if timeout_secs == 0 || timeout_secs > MAX_TIMEOUT_SECS {
                return Err(AgentTaskValidationError::InvalidTimeout {
                    max: MAX_TIMEOUT_SECS,
                });
            }
        }
        Ok(self)
    }

    pub fn validate(
        self,
        worker_timeout_limit: u64,
        server_timeout_limit: u64,
    ) -> Result<ValidatedAgentTaskRequest, AgentTaskValidationError> {
        let shaped = self.validate_shape()?;
        let timeout_secs = shaped.timeout_secs.unwrap_or(worker_timeout_limit);
        let max = worker_timeout_limit
            .min(server_timeout_limit)
            .min(MAX_TIMEOUT_SECS);
        if timeout_secs == 0 || timeout_secs > max {
            return Err(AgentTaskValidationError::InvalidTimeout { max });
        }
        Ok(ValidatedAgentTaskRequest {
            worker: shaped.worker,
            objective: shaped.objective,
            instructions: shaped.instructions,
            completion_criteria: shaped.completion_criteria,
            cwd: shaped.cwd,
            timeout_secs,
        })
    }
}

fn bounded_non_empty(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<(), AgentTaskValidationError> {
    if value.trim().is_empty() || value.len() > max {
        return Err(AgentTaskValidationError::InvalidText { field, max });
    }
    Ok(())
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AgentTaskValidationError {
    #[error("invalid worker id")]
    InvalidWorkerId,
    #[error("{field} must be non-empty and at most {max} bytes")]
    InvalidText { field: &'static str, max: usize },
    #[error("instructions violate count or byte limits")]
    InvalidInstructions,
    #[error("completion criteria violate count or byte limits")]
    InvalidCriteria,
    #[error("duplicate completion criterion id: {0}")]
    DuplicateCriterionId(String),
    #[error("cwd is invalid")]
    InvalidCwd,
    #[error("timeout_secs must be between 1 and {max}")]
    InvalidTimeout { max: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskStatus {
    Completed,
    Blocked,
    Cancelled,
    Failed,
    TimedOut,
    LaunchFailed,
    InvalidOutput,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskEvidenceKind {
    WorkerReport,
    WorkspaceChange,
    ProcessOutput,
    ExitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTaskEvidenceSource {
    AgentTask,
    WorkerProcess,
    WorkspaceObserver,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskEvidence {
    pub kind: AgentTaskEvidenceKind,
    pub source: AgentTaskEvidenceSource,
    pub summary: String,
    pub verified: bool,
}

impl AgentTaskEvidence {
    pub fn unverified(
        kind: AgentTaskEvidenceKind,
        source: AgentTaskEvidenceSource,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            source,
            summary: bound_text(summary.into(), MAX_EVIDENCE_SUMMARY_BYTES),
            verified: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTaskResult {
    pub status: AgentTaskStatus,
    pub summary: String,
    pub reported_complete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub changed_paths: Vec<PathBuf>,
    pub observation_incomplete: bool,
    pub evidence: Vec<AgentTaskEvidence>,
    pub verified: bool,
    pub approval_origin: String,
    pub worker: String,
    pub cwd: String,
    pub timeout_secs: u64,
}

impl AgentTaskResult {
    #[allow(clippy::too_many_arguments)]
    pub fn unverified(
        status: AgentTaskStatus,
        summary: impl Into<String>,
        reported_complete: bool,
        blockers: Vec<String>,
        stdout: String,
        stderr: String,
        stdout_truncated: bool,
        stderr_truncated: bool,
        exit_code: Option<i32>,
        timed_out: bool,
        changed_paths: Vec<PathBuf>,
        observation_incomplete: bool,
        mut evidence: Vec<AgentTaskEvidence>,
        approval_origin: impl Into<String>,
        worker: impl Into<String>,
        cwd: impl Into<String>,
        timeout_secs: u64,
    ) -> Self {
        evidence.truncate(MAX_EVIDENCE);
        for item in &mut evidence {
            item.verified = false;
            item.summary = bound_text(item.summary.clone(), MAX_EVIDENCE_SUMMARY_BYTES);
        }
        let failure = !matches!(status, AgentTaskStatus::Completed);
        let blockers = bound_blockers(blockers);
        Self {
            status,
            summary: bound_text(summary.into(), MAX_EVIDENCE_SUMMARY_BYTES),
            reported_complete: reported_complete && !failure,
            blockers,
            stdout,
            stderr,
            stdout_truncated,
            stderr_truncated,
            exit_code,
            timed_out,
            changed_paths,
            observation_incomplete,
            evidence,
            verified: false,
            approval_origin: approval_origin.into(),
            worker: worker.into(),
            cwd: cwd.into(),
            timeout_secs,
        }
    }
}

fn bound_blockers(mut blockers: Vec<String>) -> Vec<String> {
    blockers.truncate(MAX_BLOCKERS);
    blockers
        .into_iter()
        .map(|item| bound_text(item, MAX_BLOCKER_BYTES))
        .filter(|item| !item.trim().is_empty())
        .collect()
}

fn bound_text(mut value: String, max: usize) -> String {
    if value.len() > max {
        value.truncate(value.floor_char_boundary(max));
    }
    value
}
