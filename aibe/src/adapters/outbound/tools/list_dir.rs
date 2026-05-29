//! `list_dir` ツール。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ExploreLimitsConfig, ToolExecutionContext, ToolExecutor};

use super::tool_output::limit_tool_output;

const TRUNCATION_NOTE: &str = "\n... (list truncated: max_list_entries reached)";

pub struct ListDirTool {
    max_output_bytes: usize,
    explore: ExploreLimitsConfig,
}

impl ListDirTool {
    pub fn new(max_output_bytes: usize, explore: ExploreLimitsConfig) -> Self {
        Self {
            max_output_bytes,
            explore,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ListDirArgs {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    recursive: bool,
}

fn has_parent_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

fn entry_kind(path: &Path, metadata: &std::fs::Metadata) -> &'static str {
    if metadata.file_type().is_symlink() {
        "link"
    } else if metadata.is_dir() {
        "dir"
    } else if metadata.is_file() {
        "file"
    } else {
        let _ = path;
        "other"
    }
}

async fn collect_entries(
    root: &Path,
    recursive: bool,
    base_dir: &Path,
    max_entries: usize,
) -> Result<(Vec<String>, bool), String> {
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|e| e.to_string())?;
    let canonical_base = tokio::fs::canonicalize(base_dir)
        .await
        .map_err(|e| e.to_string())?;
    if !canonical_root.starts_with(&canonical_base) {
        return Err("path escapes client cwd".to_string());
    }
    let root_meta = tokio::fs::metadata(&canonical_root)
        .await
        .map_err(|e| e.to_string())?;
    if !root_meta.is_dir() {
        return Err("path is not a directory".to_string());
    }

    let mut stack = vec![canonical_root.clone()];
    let mut lines = Vec::new();
    let mut truncated = false;
    'walk: while let Some(dir) = stack.pop() {
        let mut read_dir = tokio::fs::read_dir(&dir).await.map_err(|e| e.to_string())?;
        let mut paths = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|e| e.to_string())? {
            paths.push(entry.path());
        }
        paths.sort();

        for path in paths {
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(|e| e.to_string())?;
            let rel = path.strip_prefix(&canonical_root).unwrap_or(&path);
            let rel = if rel.as_os_str().is_empty() {
                Path::new(".")
            } else {
                rel
            };
            let mut display = rel.display().to_string();
            let kind = entry_kind(&path, &metadata);
            if metadata.is_dir() {
                display.push('/');
            }
            lines.push(format!("{kind} {display}"));
            if lines.len() >= max_entries {
                truncated = true;
                break 'walk;
            }

            if recursive && metadata.is_dir() {
                stack.push(path);
            }
        }
    }

    lines.sort();
    Ok((lines, truncated))
}

#[async_trait]
impl ToolExecutor for ListDirTool {
    fn name(&self) -> ToolName {
        ToolName::list_dir()
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
        let parsed: ListDirArgs = match serde_json::from_value(arguments.clone()) {
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
            collect_entries(
                &target,
                parsed.recursive,
                ctx.base_dir(),
                self.explore.max_list_entries,
            ),
        )
        .await
        {
            Ok(Ok((entries, truncated))) => {
                let output = if entries.is_empty() {
                    "no entries".to_string()
                } else {
                    let mut out = entries.join("\n");
                    if truncated {
                        out.push_str(TRUNCATION_NOTE);
                    }
                    out
                };
                let output = limit_tool_output(&output, self.max_output_bytes);
                (
                    ExecutedToolCall::ok(id.clone(), self.name(), args_value, output.clone()),
                    ToolResult {
                        tool_call_id: id,
                        content: output,
                        is_error: false,
                    },
                )
            }
            Ok(Err(e)) => {
                let msg = format!("list dir failed: {e}");
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "list_failed", &msg),
                    ToolResult {
                        tool_call_id: id,
                        content: msg,
                        is_error: true,
                    },
                )
            }
            Err(_) => {
                let msg = format!("list_dir timed out after {timeout_ms}ms");
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
    async fn truncates_when_max_entries_reached() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "x").expect("write");
        }
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let limits = ExploreLimitsConfig {
            max_list_entries: 3,
            ..ExploreLimitsConfig::default()
        };
        let tool = ListDirTool::new(4096, limits);
        let (_record, result) = tool
            .execute("tc0", &serde_json::json!({}), 5000, &ctx)
            .await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("max_list_entries"));
    }

    #[tokio::test]
    async fn lists_directory_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a").expect("write");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = ListDirTool::new(4096, ExploreLimitsConfig::default());
        let (_record, result) = tool
            .execute("tc1", &serde_json::json!({}), 5000, &ctx)
            .await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.contains("file a.txt"));
        assert!(result.content.contains("dir sub/"));
    }

    #[tokio::test]
    async fn rejects_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = ListDirTool::new(4096, ExploreLimitsConfig::default());
        let args = serde_json::json!({"path": "/tmp"});
        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("relative"));
    }

    #[tokio::test]
    async fn rejects_parent_traversal_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = ToolExecutionContext::new(ClientCwd::new(dir.path().to_path_buf()).expect("cwd"));
        let tool = ListDirTool::new(4096, ExploreLimitsConfig::default());
        let args = serde_json::json!({"path": "../outside"});
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
        std::fs::write(outside.path().join("secret.txt"), "secret").expect("write secret");
        symlink(outside.path(), base.path().join("link_out")).expect("create symlink");

        let ctx =
            ToolExecutionContext::new(ClientCwd::new(base.path().to_path_buf()).expect("cwd"));
        let tool = ListDirTool::new(4096, ExploreLimitsConfig::default());
        let args = serde_json::json!({"path": "link_out"});
        let (_record, result) = tool.execute("tc4", &args, 5000, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("path escapes client cwd"));
    }
}
