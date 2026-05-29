//! セッション dir から `SessionInfo` を読む。

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
}
