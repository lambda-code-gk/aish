//! signal 送信と process 終了待ち。

use std::fs;
use std::time::{Duration, Instant};

use thiserror::Error;

pub const DEFAULT_STOP_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to send signal to pid {pid}: {reason}")]
    Signal { pid: u32, reason: String },
    #[error("timed out waiting for pid {pid} to exit after {secs}s")]
    WaitTimeout { pid: u32, secs: u64 },
}

pub fn send_sigterm(pid: u32) -> Result<(), ProcessError> {
    send_signal(pid, libc::SIGTERM)
}

pub fn send_sigkill(pid: u32) -> Result<(), ProcessError> {
    send_signal(pid, libc::SIGKILL)
}

fn send_signal(pid: u32, sig: i32) -> Result<(), ProcessError> {
    if pid == 0 {
        return Err(ProcessError::Signal {
            pid,
            reason: "invalid pid 0".into(),
        });
    }
    let rc = unsafe { libc::kill(pid as i32, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(ProcessError::Signal {
            pid,
            reason: std::io::Error::last_os_error().to_string(),
        })
    }
}

pub fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<(), ProcessError> {
    let deadline = Instant::now() + timeout;
    loop {
        if is_process_stopped(pid) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ProcessError::WaitTimeout {
                pid,
                secs: timeout.as_secs(),
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 対象 process が終了したか（zombie 含む）を判定する。
fn is_process_stopped(pid: u32) -> bool {
    if pid == 0 {
        return true;
    }
    let stat_path = format!("/proc/{pid}/stat");
    let Ok(stat) = fs::read_to_string(&stat_path) else {
        return true;
    };
    let Some(rp) = stat.rfind(')') else {
        return true;
    };
    let Some(state) = stat[rp + 2..].split_whitespace().next() else {
        return true;
    };
    matches!(state.chars().next(), Some('Z') | Some('x') | Some('X')) || !process_alive(pid)
}

fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn wait_for_zombie_child_returns_without_timeout() {
        let mut child = Command::new("true").spawn().expect("spawn");
        let pid = child.id();
        let _ = child.wait();
        assert!(is_process_stopped(pid));
        assert!(wait_for_process_exit(pid, Duration::from_secs(1)).is_ok());
    }
}
