//! assistant 本文を `/bin/sh -c` filter に pipe する。

use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterRunOutcome {
    Success {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    NonZeroExit {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    SpawnFailed {
        message: String,
        stderr: Vec<u8>,
    },
}

pub fn apply_output_filter(content: &str, filter: &str) -> FilterRunOutcome {
    run_output_filter(content, filter, "/bin/sh")
}

fn drain_pipe<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    })
}

fn run_output_filter(content: &str, filter: &str, shell: &str) -> FilterRunOutcome {
    let mut child = match Command::new(shell)
        .arg("-c")
        .arg(filter)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return FilterRunOutcome::SpawnFailed {
                message: e.to_string(),
                stderr: Vec::new(),
            };
        }
    };

    let stdout_handle = child.stdout.take().map(drain_pipe);
    let stderr_handle = child.stderr.take().map(drain_pipe);

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(content.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return FilterRunOutcome::SpawnFailed {
                message: e.to_string(),
                stderr: Vec::new(),
            };
        }
    }

    let stdout = stdout_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            return FilterRunOutcome::SpawnFailed {
                message: e.to_string(),
                stderr,
            };
        }
    };

    if status.success() {
        FilterRunOutcome::Success { stdout, stderr }
    } else {
        FilterRunOutcome::NonZeroExit {
            status,
            stdout,
            stderr,
        }
    }
}

pub fn format_filter_exit_status(status: &ExitStatus) -> String {
    status
        .code()
        .map(|code| code.to_string())
        .unwrap_or_else(|| status.to_string())
}

pub fn write_filter_streams(stdout: &[u8], stderr: &[u8]) -> Result<(), std::io::Error> {
    if !stdout.is_empty() {
        std::io::stdout().write_all(stdout)?;
    }
    if !stderr.is_empty() {
        std::io::stderr().write_all(stderr)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_content_via_sed() {
        let out = apply_output_filter("hello", "sed 's/hello/world/'");
        assert_eq!(
            out,
            FilterRunOutcome::Success {
                stdout: b"world".to_vec(),
                stderr: Vec::new(),
            }
        );
    }

    #[test]
    fn non_zero_exit_returns_stdout_and_status() {
        let out = apply_output_filter("x", "sed 's/x/y/' ; exit 3");
        match out {
            FilterRunOutcome::NonZeroExit { status, stdout, .. } => {
                assert_eq!(status.code(), Some(3));
                assert_eq!(stdout, b"y");
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn spawn_failed_with_missing_shell() {
        let out = run_output_filter("x", "cat", "/no/such/shell");
        assert!(matches!(out, FilterRunOutcome::SpawnFailed { .. }));
    }

    #[test]
    fn filter_stderr_is_captured() {
        let out = apply_output_filter("x", "echo err 1>&2; cat");
        match out {
            FilterRunOutcome::Success { stdout, stderr } => {
                assert_eq!(stdout, b"x");
                assert!(stderr.starts_with(b"err"));
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    #[test]
    fn large_content_does_not_deadlock_with_cat() {
        let content = "a".repeat(256 * 1024);
        let out = apply_output_filter(&content, "cat");
        match out {
            FilterRunOutcome::Success { stdout, stderr } => {
                assert_eq!(stdout, content.as_bytes());
                assert!(stderr.is_empty());
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }
}
