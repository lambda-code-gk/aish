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
        let mut inherited_env_values = Vec::new();
        for name in &self.config.env_allowlist {
            if let Some(os_value) = std::env::var_os(name) {
                if let Some(value) = os_value.to_str() {
                    if !value.is_empty() {
                        inherited_env_values.push(value.to_string());
                    }
                }
                command.env(name, os_value);
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
                let stdout_text =
                    redact_inherited_env_values_from_bytes(&stdout, &inherited_env_values);
                let stderr_text =
                    redact_inherited_env_values_from_bytes(&stderr, &inherited_env_values);
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
                let blockers: Vec<String> = bound_blockers(
                    report
                        .blockers
                        .into_iter()
                        .map(|item| redact_inherited_env_values(&item, &inherited_env_values))
                        .collect(),
                );
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
                    summary: redact_inherited_env_values(&report.summary, &inherited_env_values),
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

/// Replace exact values inherited via `env_allowlist` so bare credential dumps
/// cannot bypass pattern-based `sanitize_log_text`.
///
/// After byte/item truncation, a secret may survive as a trailing proper prefix.
/// Those prefixes are also replaced so fragments do not reach the parent prompt.
fn redact_inherited_env_values(text: &str, values: &[String]) -> String {
    redact_inherited_env_values_from_bytes(text.as_bytes(), values)
}

/// Redact on raw bytes **before** UTF-8 lossy conversion so a multi-byte secret
/// truncated mid-character cannot leave a recoverable prefix next to `�`.
fn redact_inherited_env_values_from_bytes(data: &[u8], values: &[String]) -> String {
    let mut secrets: Vec<&[u8]> = values
        .iter()
        .map(String::as_bytes)
        .filter(|value| !value.is_empty())
        .collect();
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    let mut out = data.to_vec();
    for secret in &secrets {
        out = replace_bytes(&out, secret, b"[REDACTED]");
    }
    for secret in &secrets {
        redact_trailing_secret_prefix_bytes(&mut out, secret);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    if needle.is_empty() || haystack.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index < haystack.len() {
        if haystack[index..].starts_with(needle) {
            out.extend_from_slice(replacement);
            index += needle.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out
}

fn redact_trailing_secret_prefix_bytes(buf: &mut Vec<u8>, secret: &[u8]) {
    if secret.is_empty() || buf.is_empty() {
        return;
    }
    let max = secret.len().saturating_sub(1).min(buf.len());
    for prefix_len in (1..=max).rev() {
        let prefix = &secret[..prefix_len];
        if buf.ends_with(prefix) {
            buf.truncate(buf.len() - prefix_len);
            buf.extend_from_slice(b"[REDACTED]");
            return;
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

#[cfg(test)]
mod tests {
    use super::{redact_inherited_env_values_from_bytes, replace_bytes};

    #[test]
    fn redacts_multibyte_secret_truncated_mid_character() {
        let secret = "key-パスワード-secret";
        let bytes = secret.as_bytes();
        let multibyte_at = secret.find('パ').expect("fixture contains パ");
        let cut = secret[..multibyte_at].len() + 1; // keep only the lead byte of パ
        let truncated = &bytes[..cut];
        assert!(
            std::str::from_utf8(truncated).is_err(),
            "fixture must end mid-character"
        );
        let redacted = redact_inherited_env_values_from_bytes(truncated, &[secret.to_string()]);
        assert!(
            redacted.contains("[REDACTED]"),
            "expected redaction marker, got {redacted:?}"
        );
        assert!(
            !redacted.contains("key-"),
            "ASCII prefix of truncated multibyte secret must not survive: {redacted:?}"
        );
    }

    #[test]
    fn replace_bytes_replaces_all_occurrences() {
        let out = replace_bytes(b"abXab", b"ab", b"Q");
        assert_eq!(out, b"QXQ");
    }
}
