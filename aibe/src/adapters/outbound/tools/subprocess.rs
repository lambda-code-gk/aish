//! subprocess の spawn / timeout / kill / reap（`shell_exec` と `git` 系ツール共通）。

use std::time::Duration;

use tokio::io::AsyncReadExt;
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
