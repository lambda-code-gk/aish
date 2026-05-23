//! `read_file` ツール。

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolResult};
use crate::ports::outbound::{ReadFileConfig, ToolExecutionContext, ToolExecutor};

use super::tool_output::limit_tool_output;

pub struct ReadFileTool {
    config: ReadFileConfig,
    max_output_bytes: usize,
}

impl ReadFileTool {
    pub fn new(config: ReadFileConfig, max_output_bytes: usize) -> Self {
        Self {
            config,
            max_output_bytes,
        }
    }

    fn resolve_allowed_roots(&self, ctx: &ToolExecutionContext) -> Vec<PathBuf> {
        self.config
            .allowed_roots
            .iter()
            .map(|p| {
                if p == Path::new(".") {
                    ctx.base_dir()
                } else {
                    expand_home_path(p)
                }
            })
            .map(|p| p.canonicalize().unwrap_or(p))
            .collect()
    }

    fn slice_lines(content: &str, offset: Option<u32>, limit: Option<u32>) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let start = offset.map(|o| o.saturating_sub(1) as usize).unwrap_or(0);
        let end = limit
            .map(|l| start.saturating_add(l as usize))
            .unwrap_or(lines.len())
            .min(lines.len());
        if start >= lines.len() {
            return String::new();
        }
        lines[start..end].join("\n")
    }

    async fn read_within_roots(
        roots: &[PathBuf],
        path: &Path,
        offset: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        let canonical = tokio::fs::canonicalize(path)
            .await
            .map_err(|e| e.to_string())?;
        if !roots.iter().any(|root| canonical.starts_with(root)) {
            return Err("path is outside allowed_roots".to_string());
        }
        let content = tokio::fs::read_to_string(&canonical)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self::slice_lines(&content, offset, limit))
    }
}

fn expand_home_path(path: &Path) -> PathBuf {
    if let Some(s) = path.to_str() {
        if let Some(rest) = s.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            return PathBuf::from(home).join(rest);
        }
    }
    path.to_path_buf()
}

fn path_has_parent_traversal(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::ParentDir))
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[async_trait]
impl ToolExecutor for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
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

        let parsed: ReadFileArgs = match serde_json::from_value(arguments.clone()) {
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

        if parsed.path.trim().is_empty() {
            let msg = "path must not be empty";
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

        let path = PathBuf::from(&parsed.path);
        if path_has_parent_traversal(&path) {
            let msg = "path must not contain '..'";
            return (
                ExecutedToolCall::err(
                    id.clone(),
                    self.name().to_string(),
                    args_value,
                    "path_not_allowed",
                    msg,
                ),
                ToolResult {
                    tool_call_id: id,
                    content: msg.to_string(),
                    is_error: true,
                },
            );
        }

        let roots = self.resolve_allowed_roots(ctx);
        let read_path = ctx.resolve_path(&path);
        let offset = parsed.offset;
        let limit = parsed.limit;
        let max_output_bytes = self.max_output_bytes;
        let duration = Duration::from_millis(timeout_ms.max(1));

        match timeout(
            duration,
            Self::read_within_roots(&roots, &read_path, offset, limit),
        )
        .await
        {
            Ok(Ok(text)) => {
                let limited = limit_tool_output(&text, max_output_bytes);
                (
                    ExecutedToolCall::ok(
                        id.clone(),
                        self.name().to_string(),
                        args_value,
                        limited.clone(),
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: limited,
                        is_error: false,
                    },
                )
            }
            Ok(Err(e)) => {
                let (code, msg) = if e == "path is outside allowed_roots" {
                    ("path_not_allowed", e)
                } else {
                    ("read_failed", format!("read failed: {e}"))
                };
                (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name().to_string(),
                        args_value,
                        code,
                        &msg,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: msg,
                        is_error: true,
                    },
                )
            }
            Err(_) => {
                let msg = format!("read timed out after {timeout_ms}ms");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::ReadFileConfig;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn relative_path_uses_client_cwd_not_server_cwd() {
        let dir = tempdir().expect("tempdir");
        let client_sub = dir.path().join("client");
        std::fs::create_dir_all(&client_sub).expect("mkdir");
        std::fs::write(client_sub.join("note.txt"), "from client cwd").expect("write");

        let tool = ReadFileTool::new(
            ReadFileConfig {
                allowed_roots: vec![dir.path().to_path_buf()],
            },
            4096,
        );
        let ctx = ToolExecutionContext::from_client_cwd(Some(client_sub.clone()));
        let args = json!({ "path": "note.txt" });

        let (_record, result) = tool.execute("tc1", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.content, "from client cwd");
    }

    #[tokio::test]
    async fn dot_allowed_root_uses_client_cwd() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("root.txt"), "dot root").expect("write");

        let tool = ReadFileTool::new(
            ReadFileConfig {
                allowed_roots: vec![PathBuf::from(".")],
            },
            4096,
        );
        let ctx = ToolExecutionContext::from_client_cwd(Some(dir.path().to_path_buf()));
        let args = json!({ "path": "root.txt" });

        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.content, "dot root");
    }
}
