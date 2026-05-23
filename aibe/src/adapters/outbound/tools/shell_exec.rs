//! `shell_exec` ツール。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolResult};
use crate::ports::outbound::{CommandPolicy, ToolExecutor};

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

#[async_trait]
impl ToolExecutor for ShellExecTool {
    fn name(&self) -> &'static str {
        "shell_exec"
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        timeout_ms: u64,
    ) -> (ExecutedToolCall, ToolResult) {
        let id = tool_call_id.to_string();
        let args_value = arguments.clone();

        if !self.policy.shell_exec_enabled() {
            let msg = "shell_exec is disabled in server config";
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name().to_string(),
                    args_value,
                    "disabled",
                    msg,
                ),
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
                        self.name().to_string(),
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
                    self.name().to_string(),
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
                    self.name().to_string(),
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

        let mut cmd = Command::new(&parsed.command);
        cmd.args(&parsed.args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let duration = Duration::from_millis(timeout_ms);
        let run = async {
            let child = cmd.spawn().map_err(|e| format!("failed to spawn: {e}"))?;
            child
                .wait_with_output()
                .await
                .map_err(|e| format!("failed to run command: {e}"))
        };

        match timeout(duration, run).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let code = output.status.code().unwrap_or(-1);
                let body_raw = format!("exit_code: {code}\nstdout:\n{stdout}\nstderr:\n{stderr}");
                let body = limit_tool_output(&body_raw, self.max_output_bytes);
                if output.status.success() {
                    (
                        ExecutedToolCall::ok(
                            id.clone(),
                            self.name().to_string(),
                            args_value,
                            body.clone(),
                        ),
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
                            self.name().to_string(),
                            args_value,
                            "nonzero_exit",
                            format!("process exited with {code}"),
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: body,
                            is_error: true,
                        },
                    )
                }
            }
            Ok(Err(msg)) => (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name().to_string(),
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
            Err(_) => {
                let msg = format!("command timed out after {timeout_ms}ms");
                (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name().to_string(),
                        args_value,
                        "timeout",
                        &msg,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.clone(),
                        is_error: true,
                    },
                )
            }
        }
    }
}
