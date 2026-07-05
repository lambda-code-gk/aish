//! Collaborative handoff 用 PTY human shell（0055 Phase 2）。

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::adapters::outbound::toml_config::AishConfig;
use crate::adapters::outbound::{
    create_shell_session, prune_old_sessions, resolve_sessions_parent, JsonlFileLog, PtyShell,
    RedactingSessionLog,
};
use crate::application::RunShell;
use crate::domain::{CommandSpec, LogEvent};
use crate::ports::outbound::SessionLog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanShellResult {
    pub normal_return: bool,
    pub exit_code: Option<i32>,
    pub final_cwd: PathBuf,
    pub shell_session_id: String,
    pub shell_session_dir: PathBuf,
    pub shell_log_start: u64,
    pub shell_log_end: u64,
}

pub const HANDOFF_ENV_KEYS: [&str; 4] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_ID",
    "AISH_HANDOFF_TOKEN",
    "AISH_HANDOFF_CONTEXT_VERSION",
];

pub fn handoff_environment_is_complete<'a>(
    values: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    let values: std::collections::HashMap<&str, &str> = values.into_iter().collect();
    values.get("AISH_CONTROL_MODE") == Some(&"human-shell")
        && HANDOFF_ENV_KEYS
            .iter()
            .all(|key| values.get(key).is_some_and(|value| !value.is_empty()))
}

pub fn human_shell_result_from_marker(
    marker: crate::adapters::outbound::HumanReturnMarker,
    child_exit_code: i32,
) -> HumanShellResult {
    HumanShellResult {
        normal_return: true,
        exit_code: marker.exit_code.or(Some(child_exit_code)),
        final_cwd: PathBuf::from(marker.final_cwd),
        shell_session_id: String::new(),
        shell_session_dir: PathBuf::new(),
        shell_log_start: 0,
        shell_log_end: 0,
    }
}

pub fn validate_handoff_environment() -> anyhow::Result<()> {
    for key in HANDOFF_ENV_KEYS.iter().skip(1) {
        if std::env::var_os(key).is_none() {
            anyhow::bail!("missing required human-shell environment variable {key}");
        }
    }
    if std::env::var("AISH_CONTROL_MODE").as_deref() != Ok("human-shell") {
        anyhow::bail!("AISH_CONTROL_MODE must be human-shell");
    }
    Ok(())
}

pub fn run_human_shell(result_file: &Path) -> anyhow::Result<HumanShellResult> {
    validate_handoff_environment()?;
    crate::collaborative_briefing::print_handoff_briefing_if_needed();
    let heartbeat = LeaseHeartbeatSupervisor::from_environment()?;
    let cfg = AishConfig::load();
    let parent = resolve_sessions_parent(&cfg);
    let layout = create_shell_session(&parent)?;
    prune_old_sessions(&parent, cfg.max_sessions)?;
    let shell = cfg.shell;
    let token = std::env::var("AISH_HANDOFF_TOKEN").unwrap_or_default();
    let secrets = if token.is_empty() {
        Vec::new()
    } else {
        vec![token]
    };
    let mut log = RedactingSessionLog::new(JsonlFileLog::new(layout.log_path.clone()), secrets);
    log.append(&LogEvent::command_start(&CommandSpec {
        program: "human_shell".into(),
        args: vec![shell.clone()],
    }))?;
    let mut runner = PtyShell::new(&mut log);
    let code = RunShell::new(&mut runner).run(&shell, &layout.dir)?;
    let marker = runner
        .take_human_return_marker()
        .ok_or_else(|| anyhow::anyhow!("human shell ended without normal return marker"))?;
    log.append(&LogEvent::Exit { code: Some(code) })?;
    let mut result = human_shell_result_from_marker(marker, code);
    result.shell_session_id = layout.id;
    result.shell_session_dir = layout.dir;
    result.shell_log_end = std::fs::metadata(&layout.log_path)
        .map(|m| m.len())
        .unwrap_or(0);
    write_result(result_file, &result)?;
    drop(heartbeat);
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LeaseFile {
    handoff_id: String,
    owner_client_id: String,
    owner_process_id: u32,
    owner_tty: Option<String>,
    owner_host: String,
    owner_uid: u32,
    lease_acquired_at_ms: u64,
    lease_expires_at_ms: u64,
    last_heartbeat_at_ms: u64,
}

/// Prompt 描画と独立して lease を更新する human-shell supervisor。
struct LeaseHeartbeatSupervisor {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LeaseHeartbeatSupervisor {
    fn from_environment() -> anyhow::Result<Option<Self>> {
        let Some(root) = std::env::var_os("AISH_HANDOFF_STORE_ROOT") else {
            return Ok(None);
        };
        let handoff_id = std::env::var("AISH_HANDOFF_ID")?;
        validate_handoff_id(&handoff_id)?;
        let interval_ms = env_u64("AISH_HANDOFF_HEARTBEAT_INTERVAL_MS", 30_000)?;
        let timeout_ms = env_u64("AISH_HANDOFF_LEASE_TIMEOUT_MS", 120_000)?;
        if interval_ms == 0 || timeout_ms <= interval_ms {
            anyhow::bail!("invalid handoff heartbeat interval/timeout");
        }
        let lease_path = PathBuf::from(root).join(handoff_id).join("lease.json");
        heartbeat_lease_file(&lease_path, timeout_ms)?;
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || loop {
            match rx.recv_timeout(Duration::from_millis(interval_ms)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if heartbeat_lease_file(&lease_path, timeout_ms).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Some(Self {
            stop: Some(tx),
            thread: Some(thread),
        }))
    }
}

impl Drop for LeaseHeartbeatSupervisor {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn heartbeat_lease_file(path: &Path, timeout_ms: u64) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let lock_path = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("lease path has no parent"))?
        .join(".lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(lock_path)?;
    if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| {
        let raw = std::fs::read_to_string(path)?;
        let mut lease: LeaseFile = serde_json::from_str(raw.trim())?;
        let expected_handoff_id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("lease path has no handoff ID"))?;
        if lease.handoff_id != expected_handoff_id {
            anyhow::bail!("lease handoff ID does not match its directory");
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if lease.lease_expires_at_ms <= now_ms {
            anyhow::bail!("handoff lease already expired");
        }
        lease.last_heartbeat_at_ms = now_ms;
        lease.lease_expires_at_ms = now_ms.saturating_add(timeout_ms);
        let temp = path.with_extension("json.heartbeat.tmp");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        serde_json::to_writer_pretty(&mut file, &lease)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    let _ = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_UN) };
    result
}

fn validate_handoff_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 128
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        anyhow::bail!("invalid handoff id");
    }
    Ok(())
}

fn env_u64(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(value) => value.parse().map_err(|_| anyhow::anyhow!("invalid {key}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn write_result(path: &Path, result: &HumanShellResult) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer(&mut file, result)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease(expires_at: u64) -> LeaseFile {
        LeaseFile {
            handoff_id: "handoff-test".into(),
            owner_client_id: "owner".into(),
            owner_process_id: 1,
            owner_tty: None,
            owner_host: "host".into(),
            owner_uid: 1000,
            lease_acquired_at_ms: 1,
            lease_expires_at_ms: expires_at,
            last_heartbeat_at_ms: 1,
        }
    }

    #[test]
    fn supervisor_heartbeat_extends_existing_lease_without_changing_owner() {
        let temp = tempfile::tempdir().unwrap();
        let handoff_dir = temp.path().join("handoff-test");
        std::fs::create_dir(&handoff_dir).unwrap();
        let path = handoff_dir.join("lease.json");
        let future = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 60_000;
        std::fs::write(&path, serde_json::to_vec(&lease(future)).unwrap()).unwrap();
        heartbeat_lease_file(&path, 120_000).unwrap();
        let updated: LeaseFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(updated.owner_client_id, "owner");
        assert!(updated.last_heartbeat_at_ms > 1);
        assert!(updated.lease_expires_at_ms > future);
    }

    #[test]
    fn supervisor_does_not_revive_expired_lease() {
        let temp = tempfile::tempdir().unwrap();
        let handoff_dir = temp.path().join("handoff-test");
        std::fs::create_dir(&handoff_dir).unwrap();
        let path = handoff_dir.join("lease.json");
        std::fs::write(&path, serde_json::to_vec(&lease(1)).unwrap()).unwrap();
        assert!(heartbeat_lease_file(&path, 120_000).is_err());
    }
}
