//! `git_diff` / `git_status` 共通（パス検証・`rev-parse`・subprocess 実行）。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::ports::outbound::ToolExecutionContext;

use super::subprocess::{run_subprocess, ShellRunOutcome};

pub(crate) fn path_from_args(
    ctx: &ToolExecutionContext,
    path: Option<&str>,
) -> Result<PathBuf, String> {
    let candidate = path
        .map(Path::new)
        .map(|p| {
            if p.is_absolute() {
                Err("path must be relative to client cwd".to_string())
            } else if p.components().any(|c| matches!(c, Component::ParentDir)) {
                Err("path must not contain '..'".to_string())
            } else {
                Ok(ctx.resolve_path(p))
            }
        })
        .transpose()?
        .unwrap_or_else(|| ctx.base_dir().to_path_buf());
    Ok(candidate)
}

pub(crate) async fn ensure_within_base_dir(path: &Path, base_dir: &Path) -> Result<(), String> {
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut probe = path.to_path_buf();
    while !tokio::fs::try_exists(&probe)
        .await
        .map_err(|e| e.to_string())?
    {
        let Some(parent) = probe.parent() else {
            return Err("path escapes client cwd".to_string());
        };
        probe = parent.to_path_buf();
    }
    let canonical_probe = tokio::fs::canonicalize(&probe)
        .await
        .map_err(|e| e.to_string())?;
    if !canonical_probe.starts_with(&canonical_base) {
        return Err("path escapes client cwd".to_string());
    }
    Ok(())
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().and_then(|rel| {
        if rel.as_os_str().is_empty() {
            None
        } else {
            Some(rel.to_path_buf())
        }
    })
}

fn git_start_dir(path: &Path) -> Result<PathBuf, String> {
    let mut start_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .ok_or_else(|| "path has no parent directory".to_string())?
            .to_path_buf()
    };
    while !start_dir.exists() {
        let Some(parent) = start_dir.parent() else {
            break;
        };
        start_dir = parent.to_path_buf();
    }
    if start_dir.is_file() {
        start_dir = start_dir
            .parent()
            .ok_or_else(|| "path has no parent directory".to_string())?
            .to_path_buf();
    }
    Ok(start_dir)
}

pub(crate) async fn git_root_for(path: &Path, timeout_ms: u64) -> Result<PathBuf, String> {
    let start_dir = git_start_dir(path)?;
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(&start_dir)
        .arg("rev-parse")
        .arg("--show-toplevel");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let duration = Duration::from_millis(timeout_ms.max(1));
    match run_subprocess(cmd, duration).await {
        ShellRunOutcome::Completed {
            exit_code,
            stdout,
            stderr,
        } => {
            if exit_code != 0 {
                return Err(String::from_utf8_lossy(&stderr).trim().to_string());
            }
            let root = String::from_utf8_lossy(&stdout).trim().to_string();
            if root.is_empty() {
                Err("git root not found".to_string())
            } else {
                Ok(PathBuf::from(root))
            }
        }
        ShellRunOutcome::TimedOut { .. } => {
            Err(format!("git rev-parse timed out after {timeout_ms}ms"))
        }
        ShellRunOutcome::Failed(msg) => Err(format!("failed to run git rev-parse: {msg}")),
    }
}

pub(crate) async fn run_git_command(
    mut cmd: Command,
    timeout_ms: u64,
    op_name: &str,
) -> Result<(Vec<u8>, Vec<u8>), GitCommandError> {
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let duration = Duration::from_millis(timeout_ms.max(1));
    match run_subprocess(cmd, duration).await {
        ShellRunOutcome::Completed {
            exit_code,
            stdout,
            stderr,
        } => {
            if exit_code != 0 {
                let msg = String::from_utf8_lossy(&stderr).trim().to_string();
                Err(GitCommandError::NonZeroExit(msg))
            } else {
                Ok((stdout, stderr))
            }
        }
        ShellRunOutcome::TimedOut { .. } => Err(GitCommandError::TimedOut(timeout_ms)),
        ShellRunOutcome::Failed(msg) => Err(GitCommandError::Failed(format!(
            "failed to run git {op_name}: {msg}"
        ))),
    }
}

pub(crate) enum GitCommandError {
    TimedOut(u64),
    Failed(String),
    NonZeroExit(String),
}

impl GitCommandError {
    pub(crate) fn user_message(&self, op_name: &str) -> String {
        match self {
            Self::TimedOut(ms) => format!("git {op_name} timed out after {ms}ms"),
            Self::Failed(msg) => msg.clone(),
            Self::NonZeroExit(msg) => msg.clone(),
        }
    }
}
