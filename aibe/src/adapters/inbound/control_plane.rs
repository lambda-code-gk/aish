//! stop / restart / status の control plane。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use crate::adapters::outbound::TomlConfig;
use crate::clap_cli::StatusFormat;
use crate::daemon::{
    default_pid_file_path, read_pid_file, remove_pid_file, remove_trusted_runtime_socket,
    send_sigkill, send_sigterm, validate_pid_record, validate_pid_record_for_paths,
    wait_for_process_exit, PidFileRecord, PidFileState, ProcessError, DEFAULT_STOP_WAIT,
};

pub const RESTART_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
struct ControlConfigSnapshot {
    config_path: PathBuf,
    socket_path: PathBuf,
}

impl ControlConfigSnapshot {
    fn load() -> anyhow::Result<Self> {
        let config = TomlConfig::load().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self {
            config_path: TomlConfig::resolve_path_for_display(),
            socket_path: config.socket_path,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonState {
    Running,
    NotRunning,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusPidFileState {
    Missing,
    Present,
    Stale,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StatusReport {
    pub state: DaemonState,
    pub pid_file_state: StatusPidFileState,
    pub pid_file_path: PathBuf,
    pub pid: Option<u32>,
    pub config_path: PathBuf,
    pub socket_path: PathBuf,
    pub socket_ping: bool,
}

pub fn run_status(format: StatusFormat) -> anyhow::Result<()> {
    let report = build_status_report()?;
    match format {
        StatusFormat::Json => {
            println!("{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}

pub fn build_status_report() -> anyhow::Result<StatusReport> {
    let snapshot = ControlConfigSnapshot::load()?;
    let pid_file_path = default_pid_file_path();
    let pid_record = read_pid_file(&pid_file_path)?;
    let socket_ping = aibe_client::ping(&snapshot.socket_path);

    let (pid_file_state, pid) = match pid_record {
        None => (StatusPidFileState::Missing, None),
        Some(record) => match validate_pid_record_for_paths(
            &record,
            Some(&snapshot.config_path),
            Some(&snapshot.socket_path),
        ) {
            PidFileState::PresentValid => (StatusPidFileState::Present, Some(record.pid)),
            PidFileState::PresentStale => (StatusPidFileState::Stale, Some(record.pid)),
            PidFileState::Missing => (StatusPidFileState::Missing, None),
        },
    };

    let state = if socket_ping {
        DaemonState::Running
    } else {
        DaemonState::NotRunning
    };

    Ok(StatusReport {
        state,
        pid_file_state,
        pid_file_path,
        pid,
        config_path: snapshot.config_path,
        socket_path: snapshot.socket_path,
        socket_ping,
    })
}

pub fn run_stop() -> anyhow::Result<()> {
    stop_daemon_without_config()
}

pub fn run_restart() -> anyhow::Result<()> {
    let snapshot = ControlConfigSnapshot::load()?;
    stop_daemon_with_config(&snapshot)?;
    spawn_daemon()?;
    wait_for_daemon_ready(&snapshot.socket_path, RESTART_READY_TIMEOUT)?;
    Ok(())
}

/// config parse 失敗時は旧 daemon に signal を送らない。
pub fn try_run_restart() -> anyhow::Result<()> {
    run_restart()
}

fn stop_daemon_without_config() -> anyhow::Result<()> {
    let pid_file_path = default_pid_file_path();
    let Some(record) = read_pid_file(&pid_file_path)? else {
        if let Some(socket_path) = optional_env_socket_path() {
            if aibe_client::ping(&socket_path) {
                return Err(missing_pid_file_but_running(&socket_path, &pid_file_path));
            }
            remove_trusted_runtime_socket(&socket_path, &socket_path);
        }
        remove_pid_file(&pid_file_path);
        return Ok(());
    };

    match validate_pid_record(&record) {
        PidFileState::PresentValid => signal_and_wait(&record)?,
        PidFileState::PresentStale | PidFileState::Missing => {}
    }

    remove_pid_file(&pid_file_path);
    if !aibe_client::ping(&record.socket_path) {
        remove_trusted_runtime_socket(&record.socket_path, &record.socket_path);
    }
    Ok(())
}

fn stop_daemon_with_config(snapshot: &ControlConfigSnapshot) -> anyhow::Result<()> {
    let pid_file_path = default_pid_file_path();
    let Some(record) = read_pid_file(&pid_file_path)? else {
        if aibe_client::ping(&snapshot.socket_path) {
            return Err(missing_pid_file_but_running(
                &snapshot.socket_path,
                &pid_file_path,
            ));
        }
        cleanup_control_artifacts(&pid_file_path, None, &snapshot.socket_path);
        return Ok(());
    };

    match validate_pid_record_for_paths(
        &record,
        Some(&snapshot.config_path),
        Some(&snapshot.socket_path),
    ) {
        PidFileState::PresentValid => signal_and_wait(&record)?,
        PidFileState::PresentStale | PidFileState::Missing => {}
    }

    cleanup_control_artifacts(
        &pid_file_path,
        Some(&record.socket_path),
        &snapshot.socket_path,
    );
    Ok(())
}

fn missing_pid_file_but_running(socket_path: &Path, pid_file_path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "aibe appears to be running at {} but pid file is missing at {}; manual intervention required",
        socket_path.display(),
        pid_file_path.display(),
    )
}

fn optional_env_socket_path() -> Option<PathBuf> {
    std::env::var_os("AIBE_SOCKET_PATH").map(PathBuf::from)
}

fn signal_and_wait(record: &PidFileRecord) -> anyhow::Result<()> {
    send_sigterm(record.pid).map_err(|e| anyhow::anyhow!("{e}"))?;
    if wait_for_process_exit(record.pid, DEFAULT_STOP_WAIT).is_err() {
        send_sigkill(record.pid).map_err(|e| anyhow::anyhow!("{e}"))?;
        wait_for_process_exit(record.pid, Duration::from_secs(5))
            .map_err(|e: ProcessError| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

fn cleanup_control_artifacts(
    pid_file_path: &Path,
    record_socket: Option<&Path>,
    trusted_socket: &Path,
) {
    remove_pid_file(pid_file_path);
    if let Some(record_socket) = record_socket {
        if !aibe_client::ping(record_socket) {
            remove_trusted_runtime_socket(record_socket, trusted_socket);
        }
    } else if !aibe_client::ping(trusted_socket) {
        remove_trusted_runtime_socket(trusted_socket, trusted_socket);
    }
}

fn spawn_daemon() -> anyhow::Result<()> {
    let bin = resolve_aibe_binary();
    Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", bin.display()))?;
    Ok(())
}

fn wait_for_daemon_ready(socket_path: &Path, timeout: Duration) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if aibe_client::ping(socket_path) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow::anyhow!(
        "aibe did not become ready at {} within {:?}",
        socket_path.display(),
        timeout
    ))
}

fn resolve_aibe_binary() -> PathBuf {
    if let Ok(p) = std::env::var("AIBE_BIN") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("aibe");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("aibe")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::ConfigLoader;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn stop_does_not_require_config_parse() {
        let dir = tempdir().expect("tempdir");
        let bad_config = dir.path().join("bad.toml");
        fs::write(&bad_config, "not valid toml [[[").expect("write");
        let prev_config = std::env::var("AIBE_CONFIG").ok();
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("AIBE_CONFIG", &bad_config);
        std::env::set_var("HOME", dir.path());

        assert!(TomlConfig::from_path(bad_config).load().is_err());
        assert!(run_stop().is_ok());

        if let Some(value) = prev_config {
            std::env::set_var("AIBE_CONFIG", value);
        } else {
            std::env::remove_var("AIBE_CONFIG");
        }
        if let Some(value) = prev_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn restart_aborts_before_signaling_on_config_parse_failure() {
        let dir = tempdir().expect("tempdir");
        let bad_config = dir.path().join("bad.toml");
        fs::write(&bad_config, "not valid toml [[[").expect("write");
        let prev = std::env::var("AIBE_CONFIG").ok();
        std::env::set_var("AIBE_CONFIG", &bad_config);

        let load_result = TomlConfig::from_path(bad_config).load();
        assert!(load_result.is_err());
        let restart_result = try_run_restart();
        assert!(restart_result.is_err());

        if let Some(value) = prev {
            std::env::set_var("AIBE_CONFIG", value);
        } else {
            std::env::remove_var("AIBE_CONFIG");
        }
    }
}
