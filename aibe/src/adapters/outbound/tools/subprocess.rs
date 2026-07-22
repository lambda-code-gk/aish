//! subprocess の spawn / timeout / kill / reap（`shell_exec` と `git` 系ツール共通）。

use std::time::Duration;

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

/// subprocess 実行結果（テスト seam: timeout 時に `child_pid` を返す）。
#[derive(Debug)]
pub(crate) enum ShellRunOutcome {
    Completed {
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut {
        /// 単体テスト seam（`run_subprocess` 直接呼び出しで reap 検証）。
        #[allow(dead_code)]
        child_pid: u32,
    },
    Failed(String),
}

/// spawn / timeout / kill / reap を担う内部ヘルパー。
///
/// stdout/stderr は `child.wait()` と並行して drain する。終了待ちのあとにだけ
/// 読むと pipe buffer が詰まり、大量出力コマンドが誤 timeout しうる。
pub(crate) async fn run_subprocess(mut cmd: Command, duration: Duration) -> ShellRunOutcome {
    cmd.kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ShellRunOutcome::Failed(format!("failed to spawn: {e}")),
    };

    let child_pid = child.id().unwrap_or(0);
    let stdout_task = tokio::spawn(drain_stdout(child.stdout.take()));
    let stderr_task = tokio::spawn(drain_stderr(child.stderr.take()));

    match timeout(duration, child.wait()).await {
        Ok(Ok(status)) => {
            let stdout = join_drain(stdout_task).await;
            let stderr = join_drain(stderr_task).await;
            ShellRunOutcome::Completed {
                exit_code: status.code().unwrap_or(-1),
                stdout,
                stderr,
            }
        }
        Ok(Err(e)) => {
            stdout_task.abort();
            stderr_task.abort();
            ShellRunOutcome::Failed(format!("failed to run command: {e}"))
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            ShellRunOutcome::TimedOut { child_pid }
        }
    }
}

#[derive(Debug)]
pub(crate) enum BoundedRunOutcome {
    Completed {
        exit_code: i32,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        stdout_truncated: bool,
        stderr_truncated: bool,
    },
    TimedOut,
    Failed,
}

/// stdin envelope を渡し、output を読みながら上限適用し、timeout 時は process group を回収する。
pub(crate) async fn run_subprocess_bounded(
    mut cmd: Command,
    stdin: Vec<u8>,
    duration: Duration,
    max_output_bytes: usize,
) -> BoundedRunOutcome {
    cmd.kill_on_drop(true);
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return BoundedRunOutcome::Failed,
    };
    let child_pid = child.id().unwrap_or(0);
    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(&stdin).await;
    }
    let stdout_task = tokio::spawn(drain_bounded(child.stdout.take(), max_output_bytes));
    let stderr_task = tokio::spawn(drain_bounded(child.stderr.take(), max_output_bytes));
    match timeout(duration, child.wait()).await {
        Ok(Ok(status)) => {
            let (stdout, stdout_truncated) = stdout_task.await.unwrap_or_default();
            let (stderr, stderr_truncated) = stderr_task.await.unwrap_or_default();
            BoundedRunOutcome::Completed {
                exit_code: status.code().unwrap_or(-1),
                stdout,
                stderr,
                stdout_truncated,
                stderr_truncated,
            }
        }
        Ok(Err(_)) => {
            stdout_task.abort();
            stderr_task.abort();
            BoundedRunOutcome::Failed
        }
        Err(_) => {
            if child_pid != 0 {
                unsafe {
                    libc::kill(-(child_pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.start_kill();
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            BoundedRunOutcome::TimedOut
        }
    }
}

async fn drain_bounded<R: AsyncRead + Unpin>(pipe: Option<R>, max: usize) -> (Vec<u8>, bool) {
    let Some(mut reader) = pipe else {
        return (Vec::new(), false);
    };
    let mut kept = Vec::with_capacity(max.min(8192));
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = max.saturating_sub(kept.len());
        let take = read.min(remaining);
        kept.extend_from_slice(&chunk[..take]);
        truncated |= take < read;
    }
    (kept, truncated)
}

async fn drain_stdout(pipe: Option<tokio::process::ChildStdout>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut reader) = pipe {
        let _ = reader.read_to_end(&mut buf).await;
    }
    buf
}

async fn drain_stderr(pipe: Option<tokio::process::ChildStderr>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut reader) = pipe {
        let _ = reader.read_to_end(&mut buf).await;
    }
    buf
}

async fn join_drain(task: tokio::task::JoinHandle<Vec<u8>>) -> Vec<u8> {
    task.await.unwrap_or_default()
}
