//! `git_diff` ツール（読み取り専用）。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::process::Command;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ToolExecutionContext, ToolExecutor};

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

fn path_from_args(ctx: &ToolExecutionContext, path: Option<&str>) -> Result<PathBuf, String> {
    let candidate = path
        .map(Path::new)
        .map(|p| {
            if p.is_absolute() {
                Err("path must be relative to client cwd".to_string())
            } else if p.components().any(|c| matches!(c, Component::ParentDir)) {
                Err("path must not contain '..'".to_string())
            } else {
                Ok(ctx.resolve_path(p))
            }
        })
        .transpose()?
        .unwrap_or_else(|| ctx.base_dir().to_path_buf());
    Ok(candidate)
}

async fn ensure_within_base_dir(path: &Path, base_dir: &Path) -> Result<(), String> {
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .map_err(|e| e.to_string())?;
    let mut probe = path.to_path_buf();
    while !tokio::fs::try_exists(&probe)
        .await
        .map_err(|e| e.to_string())?
    {
        let Some(parent) = probe.parent() else {
            return Err("path escapes client cwd".to_string());
        };
        probe = parent.to_path_buf();
    }
    let canonical_probe = tokio::fs::canonicalize(&probe)
        .await
        .map_err(|e| e.to_string())?;
    if !canonical_probe.starts_with(&canonical_base) {
        return Err("path escapes client cwd".to_string());
    }
    Ok(())
}

async fn git_root_for(path: &Path) -> Result<PathBuf, String> {
    let mut start_dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .ok_or_else(|| "path has no parent directory".to_string())?
            .to_path_buf()
    };
    while !start_dir.exists() {
        let Some(parent) = start_dir.parent() else {
            break;
        };
        start_dir = parent.to_path_buf();
    }
    if start_dir.is_file() {
        start_dir = start_dir
            .parent()
            .ok_or_else(|| "path has no parent directory".to_string())?
            .to_path_buf();
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(&start_dir)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .await
        .map_err(|e| format!("failed to run git rev-parse: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        Err("git root not found".to_string())
    } else {
        Ok(PathBuf::from(root))
    }
}

fn relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    path.strip_prefix(root).ok().and_then(|rel| {
        if rel.as_os_str().is_empty() {
            None
        } else {
            Some(rel.to_path_buf())
        }
    })
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
        let root = match git_root_for(&target).await {
            Ok(root) => root,
            Err(msg) => {
                return (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "not_a_repo", &msg),
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

        let duration = Duration::from_millis(timeout_ms.max(1));
        let output = match timeout(duration, cmd.output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                let msg = format!("failed to run git diff: {e}");
                return (
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
                );
            }
            Err(_) => {
                let msg = format!("git diff timed out after {timeout_ms}ms");
                return (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "timeout", &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg.clone(),
                        is_error: true,
                    },
                );
            }
        };

        if !output.status.success() {
            let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return (
                ExecutedToolCall::err(id.clone(), self.name(), args_value, "nonzero_exit", &msg),
                ToolResult {
                    tool_call_id: id,
                    content: msg,
                    is_error: true,
                },
            );
        }

        let body = format!(
            "repo_root: {}\n{}\n",
            root.display(),
            String::from_utf8_lossy(&output.stdout).trim_end()
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
        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .await
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "a@example.com"])
            .current_dir(root)
            .output()
            .await
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "A"])
            .current_dir(root)
            .output()
            .await
            .expect("git config name");
        Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(root)
            .output()
            .await
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(root)
            .output()
            .await
            .expect("git commit");
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
