//! `git_diff` ツール（読み取り専用）。

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ToolExecutionContext, ToolExecutor};

use super::git_common::{
    ensure_within_base_dir, git_root_for, path_from_args, relative_path, run_git_command,
    GitCommandError,
};
use super::tool_output::limit_tool_output;

pub struct GitDiffTool {
    max_output_bytes: usize,
}

impl GitDiffTool {
    pub fn new(max_output_bytes: usize) -> Self {
        Self { max_output_bytes }
    }
}

#[derive(Debug, Deserialize)]
struct GitDiffArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    staged: bool,
}

#[async_trait]
impl ToolExecutor for GitDiffTool {
    fn name(&self) -> ToolName {
        ToolName::git_diff()
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
        let parsed: GitDiffArgs = match serde_json::from_value(arguments.clone()) {
            Ok(v) => v,
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

        let target = match path_from_args(ctx, parsed.path.as_deref()) {
            Ok(path) => path,
            Err(msg) => {
                return (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name(),
                        args_value,
                        "path_not_allowed",
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
        if let Err(msg) = ensure_within_base_dir(&target, ctx.base_dir()).await {
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name(),
                    args_value,
                    "path_not_allowed",
                    &msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg,
                    is_error: true,
                },
            );
        }
        let root = match git_root_for(&target, timeout_ms).await {
            Ok(root) => root,
            Err(msg) => {
                let code = if msg.contains("timed out") {
                    "timeout"
                } else {
                    "not_a_repo"
                };
                return (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, code, &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg,
                        is_error: true,
                    },
                );
            }
        };

        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&root)
            .arg("diff")
            .arg("--no-ext-diff")
            .arg("--no-color");
        if parsed.staged {
            cmd.arg("--staged");
        }
        if let Some(rel) = relative_path(&root, &target) {
            cmd.arg("--").arg(rel);
        }

        let (stdout, _stderr) = match run_git_command(cmd, timeout_ms, "diff").await {
            Ok(out) => out,
            Err(e) => {
                let code = match &e {
                    GitCommandError::TimedOut(_) => "timeout",
                    GitCommandError::NonZeroExit(_) => "nonzero_exit",
                    GitCommandError::Failed(_) => "execution_failed",
                };
                let msg = e.user_message("diff");
                return (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, code, &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.clone(),
                        is_error: true,
                    },
                );
            }
        };

        let body = format!(
            "repo_root: {}\n{}\n",
            root.display(),
            String::from_utf8_lossy(&stdout).trim_end()
        );
        let out = limit_tool_output(&body, self.max_output_bytes);
        (
            ExecutedToolCall::ok(id.clone(), self.name(), args_value, out.clone()),
            ToolResult {
                tool_call_id: id,
                content: out,
                is_error: false,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;

    #[tokio::test]
    async fn runs_git_diff_in_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("tracked.txt"), "one\n").expect("write");
        let root = dir.path();
        tokio::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .await
            .expect("git init");
        for args in [
            &["config", "user.email", "a@example.com"][..],
            &["config", "user.name", "A"][..],
            &["add", "tracked.txt"][..],
            &["commit", "-m", "init"][..],
        ] {
            tokio::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .output()
                .await
                .expect("git setup");
        }
        std::fs::write(root.join("tracked.txt"), "one\ntwo\n").expect("modify");

        let ctx = ToolExecutionContext::new(ClientCwd::new(root.to_path_buf()).expect("cwd"));
        let tool = GitDiffTool::new(4096);
        let (_record, result) = tool
            .execute("tc1", &serde_json::json!({}), 5000, &ctx)
            .await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("diff --git"));
        assert!(result.content.contains("repo_root:"));
    }

    #[tokio::test]
    async fn rejects_absolute_path() {
        let root = std::env::current_dir().expect("cwd");
        let ctx = ToolExecutionContext::new(ClientCwd::new(root).expect("cwd abs"));
        let tool = GitDiffTool::new(4096);
        let args = serde_json::json!({"path":"/tmp"});
        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("relative"));
    }

    #[tokio::test]
    async fn rejects_parent_traversal_path() {
        let root = std::env::current_dir().expect("cwd");
        let ctx = ToolExecutionContext::new(ClientCwd::new(root).expect("cwd abs"));
        let tool = GitDiffTool::new(4096);
        let args = serde_json::json!({"path":"../tmp"});
        let (_record, result) = tool.execute("tc3", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("must not contain '..'"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_escape_path() {
        use std::os::unix::fs::symlink;

        let base = tempfile::tempdir().expect("base tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        std::fs::create_dir(base.path().join("repo")).expect("mkdir repo");
        symlink(outside.path(), base.path().join("repo").join("link_out")).expect("symlink");

        let ctx = ToolExecutionContext::new(
            ClientCwd::new(base.path().join("repo")).expect("cwd in repo dir"),
        );
        let tool = GitDiffTool::new(4096);
        let args = serde_json::json!({"path":"link_out"});
        let (_record, result) = tool.execute("tc4", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("path escapes client cwd"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_ancestor_when_leaf_missing() {
        use std::os::unix::fs::symlink;

        let base = tempfile::tempdir().expect("base tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        symlink(outside.path(), base.path().join("link_out")).expect("symlink");

        let ctx =
            ToolExecutionContext::new(ClientCwd::new(base.path().to_path_buf()).expect("cwd"));
        let tool = GitDiffTool::new(4096);
        let args = serde_json::json!({"path":"link_out/notyet"});
        let (_record, result) = tool.execute("tc5", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("path escapes client cwd"));
    }
}
