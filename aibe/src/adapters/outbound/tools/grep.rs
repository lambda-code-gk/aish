//! `grep` ツール。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ToolExecutionContext, ToolExecutor};

use super::tool_output::limit_tool_output;

pub struct GrepTool {
    max_output_bytes: usize,
}

impl GrepTool {
    pub fn new(max_output_bytes: usize) -> Self {
        Self { max_output_bytes }
    }
}

#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

fn has_parent_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

async fn grep_file(regex: &Regex, root: &Path, path: &Path) -> Result<Vec<String>, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| e.to_string())?;
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel = if rel.as_os_str().is_empty() {
        path.file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        rel.to_path_buf()
    };
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        if regex.is_match(line) {
            out.push(format!("{}:{}:{}", rel.display(), idx + 1, line));
        }
    }
    Ok(out)
}

async fn walk_and_grep(
    regex: &Regex,
    base_dir: &Path,
    path: &Path,
    recursive: bool,
) -> Result<Vec<String>, String> {
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .map_err(|e| e.to_string())?;
    let canonical_path = tokio::fs::canonicalize(path)
        .await
        .map_err(|e| e.to_string())?;
    if !canonical_path.starts_with(&canonical_base) {
        return Err("path escapes client cwd".to_string());
    }
    let metadata = tokio::fs::metadata(&canonical_path)
        .await
        .map_err(|e| e.to_string())?;
    if metadata.is_file() {
        return grep_file(regex, &canonical_base, &canonical_path).await;
    }
    if !metadata.is_dir() {
        return Err("path is not a file or directory".to_string());
    }

    let mut stack = vec![canonical_path];
    let mut lines = Vec::new();
    while let Some(dir) = stack.pop() {
        let mut read_dir = tokio::fs::read_dir(&dir).await.map_err(|e| e.to_string())?;
        let mut children = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|e| e.to_string())? {
            children.push(entry.path());
        }
        children.sort();

        for child in children {
            let meta = tokio::fs::symlink_metadata(&child)
                .await
                .map_err(|e| e.to_string())?;
            if meta.is_dir() {
                if recursive {
                    stack.push(child);
                }
                continue;
            }
            if meta.is_file() {
                if let Ok(matches) = grep_file(regex, &canonical_base, &child).await {
                    lines.extend(matches);
                }
            }
        }
    }
    Ok(lines)
}

#[async_trait]
impl ToolExecutor for GrepTool {
    fn name(&self) -> ToolName {
        ToolName::grep()
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
        let parsed: GrepArgs = match serde_json::from_value(arguments.clone()) {
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

        if parsed.pattern.trim().is_empty() {
            let msg = "pattern must not be empty";
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

        let regex = match Regex::new(&parsed.pattern) {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("invalid regex: {e}");
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

        let rel = parsed.path.unwrap_or_else(|| ".".to_string());
        let rel_path = PathBuf::from(&rel);
        if rel_path.is_absolute() {
            let msg = "path must be relative to client cwd";
            return (
                ExecutedToolCall::err(id.clone(), self.name(), args_value, "path_not_allowed", msg),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }
        if has_parent_traversal(&rel_path) {
            let msg = "path must not contain '..'";
            return (
                ExecutedToolCall::err(id.clone(), self.name(), args_value, "path_not_allowed", msg),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }

        let target = ctx.resolve_path(&rel_path);
        let duration = Duration::from_millis(timeout_ms.max(1));
        match timeout(
            duration,
            walk_and_grep(&regex, ctx.base_dir(), &target, true),
        )
        .await
        {
            Ok(Ok(lines)) => {
                let rendered = if lines.is_empty() {
                    "no matches".to_string()
                } else {
                    lines.join("\n")
                };
                let rendered = limit_tool_output(&rendered, self.max_output_bytes);
                (
                    ExecutedToolCall::ok(id.clone(), self.name(), args_value, rendered.clone()),
                    ToolResult {
                        tool_call_id: id,
                        content: rendered,
                        is_error: false,
                    },
                )
            }
            Ok(Err(e)) => {
                let msg = format!("grep failed: {e}");
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "grep_failed", &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg,
                        is_error: true,
                    },
                )
            }
            Err(_) => {
                let msg = format!("grep timed out after {timeout_ms}ms");
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "timeout", &msg),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;

    #[tokio::test]
    async fn finds_matching_lines_recursively() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("note.txt"), "alpha\nbeta\nalphabet\n").expect("write");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
        std::fs::write(dir.path().join("sub/other.txt"), "omega\nalpha\n").expect("write");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = GrepTool::new(4096);
        let args = serde_json::json!({"pattern":"alpha"});
        let (_record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("note.txt:1:alpha"));
        assert!(result.content.contains("note.txt:3:alphabet"));
        assert!(result.content.contains("sub/other.txt:2:alpha"));
    }

    #[tokio::test]
    async fn rejects_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("note.txt"), "alpha\n").expect("write");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = GrepTool::new(4096);
        let args = serde_json::json!({"pattern":"alpha","path":"/tmp/note.txt"});
        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("relative"));
    }

    #[tokio::test]
    async fn rejects_parent_traversal_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = GrepTool::new(4096);
        let args = serde_json::json!({"pattern":"alpha","path":"../note.txt"});
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
        std::fs::write(outside.path().join("secret.txt"), "alpha").expect("write secret");
        symlink(
            outside.path().join("secret.txt"),
            base.path().join("secret_link.txt"),
        )
        .expect("create symlink");

        let ctx =
            ToolExecutionContext::new(ClientCwd::new(base.path().to_path_buf()).expect("cwd"));
        let tool = GrepTool::new(4096);
        let args = serde_json::json!({"pattern":"alpha","path":"secret_link.txt"});
        let (_record, result) = tool.execute("tc4", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("path escapes client cwd"));
    }
}
