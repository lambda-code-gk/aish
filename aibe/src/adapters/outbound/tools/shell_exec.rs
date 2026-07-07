//! `shell_exec` ツール。
//!
//! subprocess の cwd は [`ToolExecutionContext::base_dir`]（クライアントの `context.cwd`）。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use crate::domain::{Capability, ExecutedToolCall, ShellExecApprovalOutcome, ToolName, ToolResult};
use crate::ports::outbound::{
    CommandPolicy, ExternalCommandConfig, ShellExecApprovalMode, ToolExecutionContext, ToolExecutor,
};
use aibe_client::validate_shell_exec_approval_decision;
use aibe_protocol::ShellExecApprovalOrigin;

use super::subprocess::{run_subprocess, ShellRunOutcome};
use super::tool_output::limit_tool_output;

pub struct ShellExecTool {
    policy: Arc<dyn CommandPolicy>,
    max_output_bytes: usize,
    external_commands: Vec<ExternalCommandConfig>,
}

impl ShellExecTool {
    pub fn new(
        policy: Arc<dyn CommandPolicy>,
        max_output_bytes: usize,
        external_commands: Vec<ExternalCommandConfig>,
    ) -> Self {
        Self {
            policy,
            max_output_bytes,
            external_commands,
        }
    }

    fn match_external_command(&self, command: &str) -> Option<&ExternalCommandConfig> {
        self.external_commands
            .iter()
            .find(|entry| entry.command == command)
    }
}

#[derive(Debug, Deserialize)]
struct ShellExecArgs {
    command: String,
    #[serde(default)]
    args: Vec<String>,
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

        if let Err(denied) = ctx.require_capability(Capability::ShellExecute) {
            let msg = denied.message();
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name(),
                    args_value,
                    "capability_denied",
                    &msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg,
                    is_error: true,
                },
            );
        }

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
            return finish_shell_exec(
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
                self.policy.shell_exec_approval_mode(),
                ShellExecApprovalOutcome::NotApplicable,
                None,
                None,
            );
        }

        let external_name = self
            .match_external_command(&parsed.command)
            .map(|entry| entry.name.as_str());
        let approval_mode = if ctx.collaborative_handoff() {
            ShellExecApprovalMode::Ask
        } else {
            self.policy.shell_exec_approval_mode()
        };
        let mut approval_origin: Option<ShellExecApprovalOrigin> = None;
        let mut approval_outcome: Option<ShellExecApprovalOutcome> = None;
        match approval_mode {
            ShellExecApprovalMode::Never => {
                let msg = "shell_exec rejected by shell_exec_approval=never";
                return finish_shell_exec(
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name(),
                        args_value,
                        "approval_denied",
                        msg,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.to_string(),
                        is_error: true,
                    },
                    approval_mode,
                    ShellExecApprovalOutcome::PolicyNever,
                    external_name,
                    None,
                );
            }
            ShellExecApprovalMode::Ask => {
                let Some(gate) = ctx.approval_gate() else {
                    let msg = "shell_exec approval required but no interactive client connected";
                    return finish_shell_exec(
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "approval_unavailable",
                            msg,
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: msg.to_string(),
                            is_error: true,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::ApprovalUnavailable,
                        external_name,
                        None,
                    );
                };
                let Some(decision) = gate
                    .request_shell_exec_approval(&id, &parsed.command, &parsed.args)
                    .await
                else {
                    let msg = "shell_exec approval required but no interactive client connected";
                    return finish_shell_exec(
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "approval_unavailable",
                            msg,
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: msg.to_string(),
                            is_error: true,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::ApprovalUnavailable,
                        external_name,
                        None,
                    );
                };
                approval_origin = Some(decision.approval_origin);
                if let Err(reason) = validate_shell_exec_approval_decision(&decision) {
                    let msg = format!("invalid shell_exec approval decision: {reason}");
                    return finish_shell_exec(
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "invalid_approval_decision",
                            &msg,
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: msg,
                            is_error: true,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::ApprovalUnavailable,
                        external_name,
                        approval_origin,
                    );
                }
                if let Some(handoff_result) = decision.handoff_result {
                    let body = serde_json::to_string(&handoff_result).unwrap_or_else(|_| {
                        "{\"execution_outcome\":\"human_control_returned\"}".into()
                    });
                    return finish_shell_exec(
                        ExecutedToolCall::ok(id.clone(), self.name(), args_value, body.clone()),
                        ToolResult {
                            tool_call_id: id,
                            content: format!(
                                "Control returned from the human shell.\n\nAISH did not automatically execute the requested command.\nThe shell exit code does not prove that the requested command ran or succeeded.\nInspect the current environment and verify the task state before continuing.\n\n{body}"
                            ),
                            is_error: false,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::CollaborativeHandoff,
                        external_name,
                        approval_origin,
                    );
                }
                if let Some(handoff_error) = decision.handoff_error {
                    let msg = handoff_error.message.clone();
                    return finish_shell_exec(
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "human_handoff_failed",
                            &msg,
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: msg,
                            is_error: true,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::CollaborativeHandoff,
                        external_name,
                        approval_origin,
                    );
                }
                if !decision.approved {
                    let msg = "shell_exec rejected by user";
                    return finish_shell_exec(
                        ExecutedToolCall::err(
                            id.clone(),
                            self.name(),
                            args_value,
                            "approval_denied",
                            msg,
                        ),
                        ToolResult {
                            tool_call_id: id,
                            content: msg.to_string(),
                            is_error: true,
                        },
                        approval_mode,
                        ShellExecApprovalOutcome::UserDenied,
                        external_name,
                        approval_origin,
                    );
                }
                approval_outcome = Some(match decision.approval_origin {
                    ShellExecApprovalOrigin::SessionAllowed
                    | ShellExecApprovalOrigin::SessionCacheExactInvocation
                    | ShellExecApprovalOrigin::SessionCacheCommandName => {
                        ShellExecApprovalOutcome::AutoApprovedSession
                    }
                    ShellExecApprovalOrigin::PatternReadOnly
                    | ShellExecApprovalOrigin::PatternMutating => {
                        ShellExecApprovalOutcome::AutoApprovedPattern
                    }
                    _ => ShellExecApprovalOutcome::UserApproved,
                });
            }
            ShellExecApprovalMode::Always => {}
        }

        let external_command = self.match_external_command(&parsed.command);
        let exec_outcome = approval_outcome.unwrap_or_else(|| exec_outcome_for_mode(approval_mode));
        let cmd = build_command(&parsed, ctx.base_dir());
        let effective_timeout_ms = external_command
            .map(|entry| entry.timeout_secs.saturating_mul(1000))
            .unwrap_or(timeout_ms)
            .max(1);
        let duration = Duration::from_millis(effective_timeout_ms);

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
                    finish_shell_exec(
                        ExecutedToolCall::ok(id.clone(), self.name(), args_value, body.clone()),
                        ToolResult {
                            tool_call_id: id,
                            content: body,
                            is_error: false,
                        },
                        approval_mode,
                        exec_outcome,
                        external_name,
                        approval_origin,
                    )
                } else {
                    finish_shell_exec(
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
                        approval_mode,
                        exec_outcome,
                        external_name,
                        approval_origin,
                    )
                }
            }
            ShellRunOutcome::TimedOut { .. } => {
                let msg = format!("command timed out after {effective_timeout_ms}ms");
                finish_shell_exec(
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "timeout", &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.clone(),
                        is_error: true,
                    },
                    approval_mode,
                    exec_outcome,
                    external_name,
                    approval_origin,
                )
            }
            ShellRunOutcome::Failed(msg) => finish_shell_exec(
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
                approval_mode,
                exec_outcome,
                external_name,
                approval_origin,
            ),
        }
    }
}

fn exec_outcome_for_mode(mode: ShellExecApprovalMode) -> ShellExecApprovalOutcome {
    match mode {
        ShellExecApprovalMode::Ask => ShellExecApprovalOutcome::UserApproved,
        ShellExecApprovalMode::Always => ShellExecApprovalOutcome::AutoApproved,
        ShellExecApprovalMode::Never => ShellExecApprovalOutcome::PolicyNever,
    }
}

fn finish_shell_exec(
    record: ExecutedToolCall,
    result: ToolResult,
    approval_mode: ShellExecApprovalMode,
    outcome: ShellExecApprovalOutcome,
    external_command: Option<&str>,
    approval_origin: Option<ShellExecApprovalOrigin>,
) -> (ExecutedToolCall, ToolResult) {
    (
        record.with_shell_exec_audit(
            approval_mode.as_str(),
            outcome,
            approval_origin,
            external_command,
        ),
        result,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::outbound::tools::subprocess::{run_subprocess, ShellRunOutcome};
    use crate::adapters::outbound::tools::ConfigAllowlistPolicy;
    use crate::adapters::outbound::StaticCapabilityPolicy;
    use crate::domain::{ClientCwd, ToolApprovalState};
    use crate::ports::outbound::{ShellExecApprovalMode, ShellExecConfig, ToolExecutionContext};
    use serde_json::json;
    use tempfile::tempdir;

    fn shell_ctx(client_cwd: ClientCwd) -> ToolExecutionContext {
        ToolExecutionContext::new(client_cwd)
            .with_capability_policy(StaticCapabilityPolicy::local_full())
    }

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
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx = shell_ctx(ClientCwd::new(client_sub).expect("absolute client cwd"));
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
    async fn never_approval_rejects_without_running() {
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Never,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"));
        let args = json!({ "command": "echo", "args": ["hi"] });

        let (record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("never"));
        assert_eq!(record.error.as_deref(), Some("approval_denied"));
        assert_eq!(
            record.approval_source.as_deref(),
            Some("shell_exec_approval=never")
        );
        assert_eq!(record.decision.as_deref(), Some("rejected_by_policy"));
    }

    #[tokio::test]
    async fn ask_without_gate_records_approval_unavailable() {
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"));
        let args = json!({ "command": "echo", "args": ["hi"] });

        let (record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert_eq!(record.error.as_deref(), Some("approval_unavailable"));
        assert_eq!(record.decision.as_deref(), Some("approval_unavailable"));
        assert_eq!(record.approval_state, Some(ToolApprovalState::NotRequired));
    }

    #[tokio::test]
    async fn ask_denied_by_gate_records_user_denied() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::ShellExecApprovalOrigin;

        struct DenyGate;

        #[async_trait]
        impl ShellExecApprovalGate for DenyGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: false,
                    approval_origin: ShellExecApprovalOrigin::UiNo,
                    handoff_result: None,
                    handoff_error: None,
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"))
                .with_turn_id("turn-1")
                .with_approval_gate(Arc::new(DenyGate));
        let args = json!({ "command": "echo", "args": ["hi"] });

        let (record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert_eq!(record.error.as_deref(), Some("approval_denied"));
        assert_eq!(record.decision.as_deref(), Some("rejected_by_user"));
        assert_eq!(
            record.approval_state,
            Some(ToolApprovalState::ExplicitClientOptIn)
        );
    }

    #[tokio::test]
    async fn external_command_sets_approval_source() {
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        }));
        let external_commands = vec![crate::ports::outbound::ExternalCommandConfig {
            name: "fixture-echo".into(),
            description: String::new(),
            command: "echo".into(),
            args: vec!["{prompt}".into()],
            timeout_secs: 30,
        }];
        let tool = ShellExecTool::new(policy, 4096, external_commands);
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"));
        let args = json!({ "command": "echo", "args": ["hi"] });

        let (record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(
            record.approval_source.as_deref(),
            Some("shell_exec_approval=always;external_command=fixture-echo")
        );
    }

    #[tokio::test]
    async fn execute_timeout_returns_error_result() {
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["sleep".into()],
            approval: ShellExecApprovalMode::Always,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"));
        let args = json!({ "command": "sleep", "args": ["5"] });

        let (record, result) = tool.execute("tc1", &args, 100, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("timed out"));
        assert_eq!(record.error.as_deref(), Some("timeout"));
    }

    #[tokio::test]
    async fn normal_shell_exec_is_unchanged() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::ShellExecApprovalOrigin;

        struct ApproveGate;

        #[async_trait]
        impl ShellExecApprovalGate for ApproveGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: true,
                    approval_origin: ShellExecApprovalOrigin::UiYes,
                    handoff_result: None,
                    handoff_error: None,
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"))
                .with_turn_id("turn-1")
                .with_approval_gate(Arc::new(ApproveGate));
        let args = json!({ "command": "echo", "args": ["ok"] });

        let (record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("exit_code: 0"));
        assert!(result.content.contains("stdout:"));
        assert!(result.content.contains("ok"));
        assert_eq!(record.error, None);
        assert_eq!(record.decision.as_deref(), Some("executed"));
    }

    #[tokio::test]
    async fn collaborative_handoff_success_becomes_synthetic_tool_result() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::{
            HandoffExecutionOutcome, HumanHandoffResult, RequestedCommandCompletion,
            ShellExecApprovalOrigin,
        };

        struct HandoffGate {
            observation_json: String,
        }

        #[async_trait]
        impl ShellExecApprovalGate for HandoffGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: true,
                    approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
                    handoff_result: Some(HumanHandoffResult {
                        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
                        requested_command: Some("'echo' 'hi'".into()),
                        requested_command_completion: RequestedCommandCompletion::Unknown,
                        human_shell_exit_code: Some(0),
                        final_shell_cwd: Some("/tmp".into()),
                        shell_log_range: None,
                        observation: Some(serde_json::from_str(&self.observation_json).unwrap()),
                    }),
                    handoff_error: None,
                })
            }
        }

        let observation = serde_json::json!({
            "cwd_exists": true,
            "cwd": "/tmp",
            "shell_log_tail": "human ran commands"
        });
        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["sleep".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx = shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("cwd"))
            .with_collaborative_handoff(true)
            .with_turn_id("turn-handoff")
            .with_approval_gate(Arc::new(HandoffGate {
                observation_json: observation.to_string(),
            }));
        let args = json!({ "command": "sleep", "args": ["30"] });

        let (record, result) = tool.execute("tc-handoff", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(record.error, None);
        assert_eq!(record.decision.as_deref(), Some("human_control_returned"));
        assert!(result.content.contains("human_control_returned"));
        assert!(result.content.contains("requested_command_completion"));
        assert!(result.content.contains("unknown"));
        assert!(result.content.contains(
            "shell exit code does not prove that the requested command ran or succeeded"
        ));
        assert!(result.content.contains("human ran commands"));
        assert!(!result.content.contains("exit_code: 0"));
    }

    #[tokio::test]
    async fn collaborative_handoff_failure_is_not_user_denial() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::{HumanHandoffFailure, ShellExecApprovalOrigin};

        struct HandoffFailGate;

        #[async_trait]
        impl ShellExecApprovalGate for HandoffFailGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: false,
                    approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
                    handoff_result: None,
                    handoff_error: Some(HumanHandoffFailure {
                        code: "human_handoff_failed".into(),
                        message: "launcher failed".into(),
                    }),
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["sleep".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx = shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("cwd"))
            .with_collaborative_handoff(true)
            .with_turn_id("turn-handoff-fail")
            .with_approval_gate(Arc::new(HandoffFailGate));
        let args = json!({ "command": "sleep", "args": ["30"] });

        let (record, result) = tool.execute("tc-handoff-fail", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert_eq!(record.error.as_deref(), Some("human_handoff_failed"));
        assert_eq!(record.decision.as_deref(), Some("human_handoff_failed"));
        assert!(!result.content.contains("rejected by user"));
        assert!(!result.content.contains("exit_code:"));
    }

    #[tokio::test]
    async fn collaborative_approved_without_result_is_rejected() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::ShellExecApprovalOrigin;

        struct MalformedGate;

        #[async_trait]
        impl ShellExecApprovalGate for MalformedGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: true,
                    approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
                    handoff_result: None,
                    handoff_error: None,
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["touch".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let dir = tempdir().expect("tempdir");
        let marker = dir.path().join("must-not-exist");
        let ctx = shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("cwd"))
            .with_collaborative_handoff(true)
            .with_turn_id("turn-malformed")
            .with_approval_gate(Arc::new(MalformedGate));
        let args = json!({
            "command": "touch",
            "args": [marker.to_string_lossy()]
        });

        let (record, result) = tool.execute("tc-malformed", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert_eq!(record.error.as_deref(), Some("invalid_approval_decision"));
        assert!(
            !marker.exists(),
            "marker command must not run on malformed approval"
        );
    }

    #[tokio::test]
    async fn handoff_failure_audit_is_not_human_control_returned() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::{HumanHandoffFailure, ShellExecApprovalOrigin};

        struct HandoffFailGate;

        #[async_trait]
        impl ShellExecApprovalGate for HandoffFailGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: false,
                    approval_origin: ShellExecApprovalOrigin::CollaborativeHandoff,
                    handoff_result: None,
                    handoff_error: Some(HumanHandoffFailure {
                        code: "human_handoff_failed".into(),
                        message: "launcher failed".into(),
                    }),
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["sleep".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx = shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("cwd"))
            .with_collaborative_handoff(true)
            .with_turn_id("turn-audit-fail")
            .with_approval_gate(Arc::new(HandoffFailGate));
        let args = json!({ "command": "sleep", "args": ["30"] });

        let (record, _result) = tool.execute("tc-audit-fail", &args, 5000, &ctx).await;
        assert_ne!(record.decision.as_deref(), Some("human_control_returned"));
        assert_eq!(record.decision.as_deref(), Some("human_handoff_failed"));
    }

    #[tokio::test]
    async fn invalid_approval_decision_is_rejected() {
        use async_trait::async_trait;
        use std::sync::Arc;

        use crate::ports::outbound::ShellExecApprovalGate;
        use aibe_client::ShellExecApprovalDecision;
        use aibe_protocol::{
            HandoffExecutionOutcome, HumanHandoffFailure, HumanHandoffResult,
            RequestedCommandCompletion, ShellExecApprovalOrigin,
        };

        struct InvalidGate;

        #[async_trait]
        impl ShellExecApprovalGate for InvalidGate {
            async fn request_shell_exec_approval(
                &self,
                _tool_call_id: &str,
                _command: &str,
                _args: &[String],
            ) -> Option<ShellExecApprovalDecision> {
                Some(ShellExecApprovalDecision {
                    approved: true,
                    approval_origin: ShellExecApprovalOrigin::UiYes,
                    handoff_result: Some(HumanHandoffResult {
                        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
                        requested_command: None,
                        requested_command_completion: RequestedCommandCompletion::Unknown,
                        human_shell_exit_code: None,
                        final_shell_cwd: None,
                        shell_log_range: None,
                        observation: None,
                    }),
                    handoff_error: Some(HumanHandoffFailure {
                        code: "human_handoff_failed".into(),
                        message: "contradiction".into(),
                    }),
                })
            }
        }

        let policy = Arc::new(ConfigAllowlistPolicy::new(ShellExecConfig {
            enabled: true,
            allowed_commands: vec!["echo".into()],
            approval: ShellExecApprovalMode::Ask,
            ..Default::default()
        }));
        let tool = ShellExecTool::new(policy, 4096, Vec::new());
        let ctx =
            shell_ctx(ClientCwd::new(std::env::current_dir().expect("cwd")).expect("absolute cwd"))
                .with_turn_id("turn-invalid")
                .with_approval_gate(Arc::new(InvalidGate));
        let args = json!({ "command": "echo", "args": ["hi"] });

        let (record, result) = tool.execute("tc-invalid", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert_eq!(record.error.as_deref(), Some("invalid_approval_decision"));
        assert!(result
            .content
            .contains("invalid shell_exec approval decision"));
    }
}
