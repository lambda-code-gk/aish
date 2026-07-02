//! `read_file` ツール。

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ReadFileConfig, ToolExecutionContext, ToolExecutor};

use super::safe_path::ReadPathPolicy;
use super::tool_output::limit_tool_output;

pub struct ReadFileTool {
    path_policy: ReadPathPolicy,
    max_output_bytes: usize,
}

impl ReadFileTool {
    pub fn new(config: ReadFileConfig, max_output_bytes: usize) -> Self {
        Self {
            path_policy: ReadPathPolicy::from_config(&config),
            max_output_bytes,
        }
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

    async fn read_canonical(
        canonical: &Path,
        offset: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        let content = tokio::fs::read_to_string(canonical)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self::slice_lines(&content, offset, limit))
    }
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[async_trait]
impl ToolExecutor for ReadFileTool {
    fn name(&self) -> ToolName {
        ToolName::read_file()
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

        if parsed.path.trim().is_empty() {
            let msg = "path must not be empty";
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

        let path = match ReadPathPolicy::validate_path_string(&parsed.path) {
            Ok(path) => path,
            Err(err) => {
                return (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name(),
                        args_value,
                        err.code,
                        &err.message,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: err.message,
                        is_error: true,
                    },
                );
            }
        };

        let offset = parsed.offset;
        let limit = parsed.limit;
        let max_output_bytes = self.max_output_bytes;
        let duration = Duration::from_millis(timeout_ms.max(1));

        let resolve = self.path_policy.resolve_read_path(ctx, &path).await;
        let canonical = match resolve {
            Ok(path) => path,
            Err(err) => {
                return (
                    ExecutedToolCall::err(
                        id.clone(),
                        self.name(),
                        args_value,
                        err.code,
                        &err.message,
                    ),
                    ToolResult {
                        tool_call_id: id,
                        content: err.message,
                        is_error: true,
                    },
                );
            }
        };

        match timeout(duration, Self::read_canonical(&canonical, offset, limit)).await {
            Ok(Ok(text)) => {
                let limited = limit_tool_output(&text, max_output_bytes);
                (
                    ExecutedToolCall::ok(id.clone(), self.name(), args_value, limited.clone()),
                    ToolResult {
                        tool_call_id: id,
                        content: limited,
                        is_error: false,
                    },
                )
            }
            Ok(Err(e)) => {
                let msg = format!("read failed: {e}");
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, "read_failed", &msg),
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
    use crate::ports::outbound::ReadFileConfig;
    use serde_json::json;
    use std::path::PathBuf;
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
        use crate::domain::ClientCwd;
        let ctx = ToolExecutionContext::new(
            ClientCwd::new(client_sub.clone()).expect("absolute client cwd"),
        );
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
        use crate::domain::ClientCwd;
        let ctx = ToolExecutionContext::new(
            ClientCwd::new(dir.path().to_path_buf()).expect("absolute client cwd"),
        );
        let args = json!({ "path": "root.txt" });

        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.content, "dot root");
    }
}
