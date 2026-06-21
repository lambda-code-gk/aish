//! PID file の read/write と stale 判定。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_PID_DIR: &str = ".local/share/aibe";
const PID_FILE_NAME: &str = "run.pid";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PidFileRecord {
    pub pid: u32,
    pub config_path: PathBuf,
    pub socket_path: PathBuf,
    /// `/proc/<pid>/stat` の starttime（jiffies since boot）。
    pub process_start_jiffies: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidFileState {
    Missing,
    PresentValid,
    PresentStale,
}

#[derive(Debug, Error)]
pub enum PidFileError {
    #[error("io: {0}")]
    Io(String),
    #[error("invalid pid file: {0}")]
    Invalid(String),
}

pub fn default_pid_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    default_pid_file_path_for_home(Path::new(&home))
}

pub fn default_pid_file_path_for_home(home: &Path) -> PathBuf {
    home.join(DEFAULT_PID_DIR).join(PID_FILE_NAME)
}

pub fn runtime_share_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(DEFAULT_PID_DIR)
}

/// aibe の runtime 領域（`~/.local/share/aibe/`）配下の socket のみ trusted とみなす。
pub fn is_trusted_runtime_socket(path: &Path) -> bool {
    let root = runtime_share_dir();
    path.strip_prefix(&root).is_ok_and(|rel| {
        rel.components().count() == 1 && rel.as_os_str() != "" && !path.ends_with(PID_FILE_NAME)
    })
}

/// 現行 config の socket と一致し、かつ trusted runtime 配下の場合のみ削除する。
pub fn remove_trusted_runtime_socket(record_socket: &Path, trusted_socket: &Path) {
    if record_socket == trusted_socket && is_trusted_runtime_socket(record_socket) {
        let _ = fs::remove_file(record_socket);
    }
}

pub fn write_pid_file(path: &Path, record: &PidFileRecord) -> Result<(), PidFileError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| PidFileError::Io(format!("{}: {e}", parent.display())))?;
    }
    let json =
        serde_json::to_string_pretty(record).map_err(|e| PidFileError::Invalid(e.to_string()))?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(|e| PidFileError::Io(format!("{}: {e}", path.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| PidFileError::Io(format!("{}: {e}", path.display())))?;
    }
    file.write_all(json.as_bytes())
        .map_err(|e| PidFileError::Io(format!("{}: {e}", path.display())))?;
    file.write_all(b"\n")
        .map_err(|e| PidFileError::Io(format!("{}: {e}", path.display())))?;
    Ok(())
}

pub fn read_pid_file(path: &Path) -> Result<Option<PidFileRecord>, PidFileError> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)
        .map_err(|e| PidFileError::Io(format!("{}: {e}", path.display())))?;
    let record: PidFileRecord = serde_json::from_str(raw.trim())
        .map_err(|e| PidFileError::Invalid(format!("{}: {e}", path.display())))?;
    Ok(Some(record))
}

pub fn remove_pid_file(path: &Path) {
    let _ = fs::remove_file(path);
}

pub fn cleanup_runtime_artifacts(pid_file_path: &Path, socket_path: &Path) {
    remove_pid_file(pid_file_path);
    let _ = fs::remove_file(socket_path);
}

pub fn cleanup_stale_pid_file_before_start(trusted_socket: &Path) {
    let pid_file_path = default_pid_file_path();
    let Ok(Some(record)) = read_pid_file(&pid_file_path) else {
        return;
    };
    if matches!(validate_pid_record(&record), PidFileState::PresentStale) {
        remove_pid_file(&pid_file_path);
        remove_trusted_runtime_socket(&record.socket_path, trusted_socket);
    }
}

pub fn build_current_pid_record(
    config_path: PathBuf,
    socket_path: PathBuf,
) -> Result<PidFileRecord, PidFileError> {
    Ok(PidFileRecord {
        pid: std::process::id(),
        config_path,
        socket_path,
        process_start_jiffies: current_process_start_jiffies()?,
    })
}

pub fn validate_pid_record(record: &PidFileRecord) -> PidFileState {
    validate_pid_record_for_paths(record, None, None)
}

pub fn validate_pid_record_for_paths(
    record: &PidFileRecord,
    expected_config_path: Option<&Path>,
    expected_socket_path: Option<&Path>,
) -> PidFileState {
    if let Some(expected) = expected_config_path {
        if record.config_path != expected {
            return PidFileState::PresentStale;
        }
    }
    if let Some(expected) = expected_socket_path {
        if record.socket_path != expected {
            return PidFileState::PresentStale;
        }
    }
    if !process_alive(record.pid) {
        return PidFileState::PresentStale;
    }
    if !is_aibe_process(record.pid) {
        return PidFileState::PresentStale;
    }
    match process_start_jiffies(record.pid) {
        Some(jiffies) if jiffies == record.process_start_jiffies => PidFileState::PresentValid,
        _ => PidFileState::PresentStale,
    }
}

pub fn current_process_start_jiffies() -> Result<u64, PidFileError> {
    process_start_jiffies(std::process::id()).ok_or_else(|| {
        PidFileError::Invalid(format!(
            "failed to read starttime for pid {}",
            std::process::id()
        ))
    })
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn is_aibe_process(pid: u32) -> bool {
    let path = format!("/proc/{pid}/cmdline");
    let Ok(raw) = fs::read(&path) else {
        return false;
    };
    let cmdline = String::from_utf8_lossy(&raw);
    cmdline.split('\0').any(|part| part.contains("aibe"))
}

fn process_start_jiffies(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rp = stat.rfind(')')?;
    let rest = stat[rp + 2..].split_whitespace().collect::<Vec<_>>();
    rest.get(19)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn pid_file_roundtrip_preserves_identity_and_metadata() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("run.pid");
        let record = PidFileRecord {
            pid: std::process::id(),
            config_path: PathBuf::from("/tmp/aibe.toml"),
            socket_path: PathBuf::from("/tmp/run.sock"),
            process_start_jiffies: current_process_start_jiffies().expect("starttime"),
        };
        write_pid_file(&path, &record).expect("write");
        let read = read_pid_file(&path).expect("read").expect("some");
        assert_eq!(read, record);
    }

    #[test]
    fn remove_trusted_runtime_socket_only_when_path_matches_and_is_runtime() {
        let dir = tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let runtime = home.join(".local/share/aibe");
        fs::create_dir_all(&runtime).expect("runtime");
        let socket = runtime.join("run.sock");
        fs::write(&socket, b"").expect("socket");
        let outside = dir.path().join("outside.sock");
        fs::write(&outside, b"").expect("outside");

        std::env::set_var("HOME", &home);
        remove_trusted_runtime_socket(&socket, &socket);
        assert!(!socket.exists());
        fs::write(&outside, b"").expect("outside again");
        remove_trusted_runtime_socket(&outside, &outside);
        assert!(outside.exists());
    }

    #[test]
    fn pid_file_detects_stale_or_mismatched_identity() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("run.pid");
        let record = PidFileRecord {
            pid: std::process::id(),
            config_path: PathBuf::from("/tmp/aibe.toml"),
            socket_path: PathBuf::from("/tmp/run.sock"),
            process_start_jiffies: 0,
        };
        write_pid_file(&path, &record).expect("write");
        let read = read_pid_file(&path).expect("read").expect("some");
        assert_eq!(validate_pid_record(&read), PidFileState::PresentStale);
        assert_eq!(
            validate_pid_record_for_paths(
                &read,
                Some(Path::new("/other/config.toml")),
                Some(Path::new("/tmp/run.sock")),
            ),
            PidFileState::PresentStale
        );
    }
}
