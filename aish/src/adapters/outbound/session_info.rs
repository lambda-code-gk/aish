//! replay 用のログパス解決と `current_log` の安全検証。

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::domain::SessionInfo;

#[derive(Debug, thiserror::Error)]
pub enum SessionReadError {
    #[error("AISH_SESSION_DIR is not set")]
    EnvNotSet,
    #[error("session directory not found: {0}")]
    NotFound(String),
    #[error("invalid session directory: {0}")]
    Invalid(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayLogResolveError {
    #[error(transparent)]
    Session(#[from] SessionReadError),
    #[error("--log PATH is required when AISH_SESSION_DIR is not set")]
    LogPathRequired,
    #[error("log path is not a regular file: {0}")]
    NotRegularFile(String),
    #[error("log path is unreadable: {0}: {1}")]
    Unreadable(String, String),
    #[error("current_log resolves outside AISH_SESSION_DIR")]
    SymlinkEscape,
}

pub fn session_dir_from_env() -> Result<PathBuf, SessionReadError> {
    std::env::var("AISH_SESSION_DIR")
        .map(PathBuf::from)
        .map_err(|_| SessionReadError::EnvNotSet)
}

pub fn read_session_info(session_dir: &Path) -> Result<SessionInfo, SessionReadError> {
    let display = session_dir.display().to_string();
    let session_dir = session_dir
        .canonicalize()
        .map_err(|_| SessionReadError::NotFound(display.clone()))?;

    let session_id = session_dir
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or(SessionReadError::Invalid(display))?
        .to_string();

    let log_file = session_dir.join("log.jsonl");
    let current_log = session_dir.join("current_log");

    if !log_file.is_file() {
        return Err(SessionReadError::Invalid(format!(
            "missing log file: {}",
            log_file.display()
        )));
    }
    if !current_log.exists() {
        return Err(SessionReadError::Invalid(format!(
            "missing current_log: {}",
            current_log.display()
        )));
    }

    Ok(SessionInfo {
        session_id,
        session_dir: session_dir.display().to_string(),
        log_file: log_file.display().to_string(),
        current_log: current_log.display().to_string(),
    })
}

pub fn resolve_replay_log_path(log_cli: Option<&Path>) -> Result<PathBuf, ReplayLogResolveError> {
    if let Some(path) = log_cli {
        return open_explicit_log_path(path);
    }
    let session_dir = session_dir_from_env().map_err(ReplayLogResolveError::Session)?;
    open_session_current_log(&session_dir, "current_log")
}

fn open_explicit_log_path(path: &Path) -> Result<PathBuf, ReplayLogResolveError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        ReplayLogResolveError::Unreadable(path.display().to_string(), e.to_string())
    })?;
    if !meta.is_file() {
        return Err(ReplayLogResolveError::NotRegularFile(
            path.display().to_string(),
        ));
    }
    File::open(path).map_err(|e| {
        ReplayLogResolveError::Unreadable(path.display().to_string(), e.to_string())
    })?;
    Ok(path.to_path_buf())
}

fn open_session_current_log(
    session_dir: &Path,
    link_name: &str,
) -> Result<PathBuf, ReplayLogResolveError> {
    let session_dir = session_dir.canonicalize().map_err(|e| {
        ReplayLogResolveError::Unreadable(session_dir.display().to_string(), e.to_string())
    })?;
    let current_log = session_dir.join(link_name);

    let meta = std::fs::metadata(&current_log).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ReplayLogResolveError::Unreadable(current_log.display().to_string(), "not found".into())
        } else {
            ReplayLogResolveError::Unreadable(current_log.display().to_string(), e.to_string())
        }
    })?;
    if meta.is_dir() {
        return Err(ReplayLogResolveError::NotRegularFile(
            current_log.display().to_string(),
        ));
    }

    let resolved = current_log.canonicalize().map_err(|e| {
        ReplayLogResolveError::Unreadable(current_log.display().to_string(), e.to_string())
    })?;
    if !resolved.starts_with(&session_dir) {
        return Err(ReplayLogResolveError::SymlinkEscape);
    }

    if !resolved.is_file() {
        return Err(ReplayLogResolveError::NotRegularFile(
            resolved.display().to_string(),
        ));
    }

    File::open(&resolved).map_err(|e| {
        ReplayLogResolveError::Unreadable(resolved.display().to_string(), e.to_string())
    })?;

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn reads_layout_from_dir() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = root.path().join("002f15d02b54");
        fs::create_dir(&session).expect("mkdir");
        fs::write(session.join("log.jsonl"), "x").expect("write");
        symlink("log.jsonl", session.join("current_log")).expect("link");

        let info = read_session_info(&session).expect("read");
        assert_eq!(info.session_id, "002f15d02b54");
        assert!(info.log_file.ends_with("log.jsonl"));
    }

    #[test]
    fn replay_current_log_resolution_rejects_escape() {
        let root = tempfile::tempdir().expect("tempdir");
        let session = root.path().join("002f15d02b54");
        fs::create_dir(&session).expect("mkdir");
        fs::write(root.path().join("outside.jsonl"), "x").expect("write outside");
        symlink("../outside.jsonl", session.join("current_log")).expect("escape link");

        let err = open_session_current_log(&session, "current_log").expect_err("escape");
        assert!(matches!(err, ReplayLogResolveError::SymlinkEscape));
    }
}
