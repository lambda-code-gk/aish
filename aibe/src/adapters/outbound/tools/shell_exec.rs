//! `shell_exec` ツール。
//!
//! subprocess の cwd は [`ToolExecutionContext::base_dir`]（クライアントの `context.cwd`）。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{CommandPolicy, ToolExecutionContext, ToolExecutor};

use super::tool_output::limit_tool_output;

pub struct ShellExecTool {
    policy: Arc<dyn CommandPolicy>,
    max_output_bytes: usize,
}

impl ShellExecTool {
    pub fn new(policy: Arc<dyn CommandPolicy>, max_output_bytes: usize) -> Self {
        Self {
            policy,
            max_output_bytes,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ShellExecArgs {
    command: String,
    #[serde(default)]
    args: Vec<String>,
}

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

fn build_command(parsed: &ShellExecArgs, cwd: &Path) -> Command {
    let mut cmd = Command::new(&parsed.command);
    cmd.args(&parsed.args);
    cmd.current_dir(cwd);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd
}

#[async_trait]
impl ToolExecutor for ShellExecTool {
    fn name(&self) -> ToolName {
        ToolName::shell_exec()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        let id = tool_call_id.to_string();
        let args_value = arguments.clone();

        if !self.policy.shell_exec_enabled() {
            let msg = "shell_exec is disabled in server config";
            return (
                ExecutedToolCall::err(id.clone(), self.name(), args_value, "disabled", msg),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }

        let parsed: ShellExecArgs = match serde_json::from_value(arguments.clone()) {
            Ok(a) => a,
            Err(e) => {
                let msg = format!("invalid arguments: {e}");
                return (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name(),
                        args_value,
                        "invalid_arguments",
                        &msg,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: msg,
                        is_error: true,
                    },
                );
            }
        };

        if parsed.command.trim().is_empty() {
            let msg = "command must not be empty";
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name(),
                    args_value,
                    "invalid_arguments",
                    msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }

        if !self.policy.is_command_allowed(&parsed.command) {
            let msg = "command not in allowed_commands";
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name(),
                    args_value,
                    "command_not_allowed",
                    msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }

        let cmd = build_command(&parsed, ctx.base_dir());
        let duration = Duration::from_millis(timeout_ms.max(1));

        match run_subprocess(cmd, duration).await {
            ShellRunOutcome::Completed {
                exit_code,
                stdout,
                stderr,
            } => {
                let stdout = String::from_utf8_lossy(&stdout);
                let stderr = String::from_utf8_lossy(&stderr);
                let body_raw =
                    format!("exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}");
                let body = limit_tool_output(&body_raw, self.max_output_bytes);
                if exit_code == 0 {
                    (
                        ExecutedToolCall::ok(id.clone(), self.name(), args_value, body.clone()),
                        ToolResult {
                            tool_call_id: id,
                            content: body,
                            is_error: false,
                        },
                    )
                } else {
                    (
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "nonzero_exit",
                            format!("process exited with {exit_code}"),
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: body,
                            is_error: true,
                        },
                    )
                }
            }
            ShellRunOutcome::TimedOut { .. } => {
                let msg = format!("command timed out after {timeout_ms}ms");
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "timeout", &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.clone(),
                        is_error: true,
                    },
                )
            }
            ShellRunOutcome::Failed(msg) => (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name(),
                    args_value,
                    "execution_failed",
                    &msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg,
                    is_error: true,
                },
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::tools::ConfigAllowlistPolicy;
    use crate::domain::ClientCwd;
    use crate::ports::outbound::ShellExecConfig;
    use serde_json::json;
    use tempfile::tempdir;

    fn process_reaped(pid: u32) -> bool {
        if pid == 0 {
            return true;
        }
        unsafe {
            if libc::kill(pid as i32, 0) == 0 {
                return false;
            }
            std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        }
    }

    #[tokio::test]
    async fn command_runs_in_client_cwd() {
        let dir = tempdir().expect("tempdir");
        let client_sub = dir.path().join("work");
        std::fs::create_dir_all(&client_sub).expect("mkdir");
        std::fs::write(client_sub.join("note.txt"), "from client cwd").expect("write");

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["cat".into()],
        }));
        let tool = ShellExecTool::new(policy, 4096);
        let ctx =
            ToolExecutionContext::new(ClientCwd::new(client_sub).expect("absolute client cwd"));
        let args = json!({ "command": "cat", "args": ["note.txt"] });

        let (_record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("from client cwd"));
    }

    #[tokio::test]
    async fn timeout_kills_and_reaps_child() {
        let cwd = std::env::current_dir().expect("cwd");
        let cmd = build_command(
            &ShellExecArgs {
                command: "sleep".into(),
                args: vec!["5".into()],
            },
            &cwd,
        );

        let outcome = run_subprocess(cmd, Duration::from_millis(100)).await;
        let ShellRunOutcome::TimedOut { child_pid } = outcome else {
            panic!("expected timeout, got {outcome:?}");
        };
        assert!(child_pid > 0, "child pid should be recorded");
        assert!(
            process_reaped(child_pid),
            "child pid {child_pid} should be reaped after timeout"
        );
    }

    #[tokio::test]
    async fn large_stdout_completes_without_false_timeout() {
        let cwd = std::env::current_dir().expect("cwd");
        let cmd = build_command(
            &ShellExecArgs {
                command: "sh".into(),
                args: vec!["-c".into(), "head -c 131072 /dev/zero".into()],
            },
            &cwd,
        );

        let outcome = run_subprocess(cmd, Duration::from_secs(5)).await;
        let ShellRunOutcome::Completed {
            exit_code,
            stdout,
            stderr,
        } = outcome
        else {
            panic!("expected completed, got {outcome:?}");
        };
        assert_eq!(exit_code, 0);
        assert_eq!(stdout.len(), 131_072);
        assert!(stderr.is_empty());
    }

    #[tokio::test]
    async fn execute_timeout_returns_error_result() {
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["sleep".into()],
        }));
        let tool = ShellExecTool::new(policy, 4096);
        let ctx = ToolExecutionContext::new(
            ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"),
        );
        let args = json!({ "command": "sleep", "args": ["5"] });

        let (record, result) = tool.execute("tc1", &args, 100, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
        assert_eq!(record.error.as_deref(), Some("timeout"));
    }
}
