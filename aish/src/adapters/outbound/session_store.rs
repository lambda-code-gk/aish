//! セッションディレクトリの作成・掃除。

use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::adapters::outbound::toml_config::AishConfig;
use crate::domain::session_id::{format_session_id, ms_since_2020, next_available_ms};

#[derive(Debug, Clone)]
pub struct SessionLayout {
    pub id: String,
    pub dir: PathBuf,
    pub log_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    #[error("failed to create session directory: {path}: {reason}")]
    CreateDir { path: PathBuf, reason: String },
    #[error("failed to create current_log symlink: {path}: {reason}")]
    CreateSymlink { path: PathBuf, reason: String },
    #[error("session id collision after regeneration")]
    IdCollision,
    #[error("failed to prune old sessions: {0}")]
    Prune(String),
}

/// config `log_dir` → 既定。
pub fn resolve_sessions_parent(cfg: &AishConfig) -> PathBuf {
    cfg.log_dir.clone()
}

/// 12 桁小文字 hex のセッション dir 名のみ対象に、辞書順で古いものから削除する。
pub fn prune_old_sessions(parent: &Path, max_sessions: usize) -> Result<(), SessionStoreError> {
    if max_sessions == 0 {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|e| SessionStoreError::Prune(e.to_string()))?;

    let mut names: Vec<String> = fs::read_dir(parent)
        .map_err(|e| SessionStoreError::Prune(e.to_string()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_managed_session_name(n))
        .collect();
    names.sort();
    if names.len() <= max_sessions {
        return Ok(());
    }
    let remove_count = names.len() - max_sessions;
    for name in names.into_iter().take(remove_count) {
        let path = parent.join(&name);
        fs::remove_dir_all(&path).map_err(|e| SessionStoreError::Prune(e.to_string()))?;
    }
    Ok(())
}

pub fn create_shell_session(parent: &Path) -> Result<SessionLayout, SessionStoreError> {
    fs::create_dir_all(parent).map_err(|e| SessionStoreError::CreateDir {
        path: parent.to_path_buf(),
        reason: e.to_string(),
    })?;

    let start_ms = ms_since_2020(SystemTime::now());
    let ms = next_available_ms(start_ms, |ms| parent.join(format_session_id(ms)).exists());
    if parent.join(format_session_id(ms)).exists() {
        return Err(SessionStoreError::IdCollision);
    }

    let id = format_session_id(ms);
    let dir = parent.join(&id);
    fs::create_dir(&dir).map_err(|e| SessionStoreError::CreateDir {
        path: dir.clone(),
        reason: e.to_string(),
    })?;

    let log_path = dir.join("log.jsonl");
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&log_path)
        .map_err(|e| SessionStoreError::CreateDir {
            path: log_path.clone(),
            reason: e.to_string(),
        })?;

    let link = dir.join("current_log");
    std::os::unix::fs::symlink("log.jsonl", &link).map_err(|e| {
        SessionStoreError::CreateSymlink {
            path: link,
            reason: e.to_string(),
        }
    })?;

    Ok(SessionLayout { id, dir, log_path })
}

fn is_managed_session_name(name: &str) -> bool {
    name.len() == 12
        && name
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_newest_by_name_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        for id in ["000000000001", "000000000002", "000000000003"] {
            fs::create_dir(dir.path().join(id)).expect("mkdir");
        }
        prune_old_sessions(dir.path(), 2).expect("prune");
        assert!(!dir.path().join("000000000001").exists());
        assert!(dir.path().join("000000000002").exists());
        assert!(dir.path().join("000000000003").exists());
    }

    #[test]
    fn create_session_has_log_and_symlink() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = create_shell_session(dir.path()).expect("create");
        assert!(layout.log_path.is_file());
        assert!(layout.dir.join("current_log").exists());
    }
}
