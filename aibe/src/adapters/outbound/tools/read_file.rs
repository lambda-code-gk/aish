//! `read_file` ツール。

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use crate::domain::{
    detect_line_ending, has_trailing_newline, sha256_hex, validate_utf8_bytes, ExecutedToolCall,
    FileTextError, ToolName, ToolResult,
};
use crate::ports::outbound::{ReadFileConfig, ToolExecutionContext, ToolExecutor};

use super::safe_path::ReadPathPolicy;
use super::tool_output::{limit_tool_output, limit_tool_output_with_metadata};

pub const FILE_METADATA_PREFIX: &str = "[aibe_file_metadata]";

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

    fn format_metadata_line(path: &str, bytes: &[u8], content: &str) -> String {
        let metadata = serde_json::json!({
            "path": path,
            "sha256": sha256_hex(bytes),
            "size_bytes": bytes.len(),
            "line_ending": detect_line_ending(content).as_str(),
            "trailing_newline": has_trailing_newline(bytes),
        });
        format!("{FILE_METADATA_PREFIX} {metadata}")
    }

    fn file_text_error(err: FileTextError) -> String {
        err.code().to_string()
    }

    async fn read_canonical(
        canonical: &Path,
        offset: Option<u32>,
        limit: Option<u32>,
        include_metadata: bool,
        path_for_metadata: &str,
    ) -> Result<String, String> {
        if include_metadata {
            let bytes = tokio::fs::read(canonical)
                .await
                .map_err(|e| e.to_string())?;
            let content = validate_utf8_bytes(&bytes).map_err(Self::file_text_error)?;
            let body = Self::slice_lines(&content, offset, limit);
            let metadata_line = Self::format_metadata_line(path_for_metadata, &bytes, &content);
            Ok(format!("{metadata_line}\n{body}"))
        } else {
            let content = tokio::fs::read_to_string(canonical)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Self::slice_lines(&content, offset, limit))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFileArgs {
    path: String,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    include_metadata: bool,
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
        let include_metadata = parsed.include_metadata;
        let path_for_metadata = parsed.path.clone();
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

        match timeout(
            duration,
            Self::read_canonical(
                &canonical,
                offset,
                limit,
                include_metadata,
                &path_for_metadata,
            ),
        )
        .await
        {
            Ok(Ok(text)) => {
                let limited = if include_metadata {
                    let (metadata_line, body) = text.split_once('\n').unwrap_or((&text, ""));
                    limit_tool_output_with_metadata(metadata_line, body, max_output_bytes)
                } else {
                    limit_tool_output(&text, max_output_bytes)
                };
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
                let is_text_error = e == FileTextError::InvalidUtf8.code()
                    || e == FileTextError::BinaryFileNotSupported.code();
                let msg = if is_text_error {
                    e
                } else {
                    format!("read failed: {e}")
                };
                let code = if is_text_error {
                    msg.as_str()
                } else {
                    "read_failed"
                };
                (
                    ExecutedToolCall::err(id.clone(), self.name(), args_value, code, &msg),
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
    use crate::domain::ClientCwd;
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
        let ctx = ToolExecutionContext::new(
            ClientCwd::new(dir.path().to_path_buf()).expect("absolute client cwd"),
        );
        let args = json!({ "path": "root.txt" });

        let (_record, result) = tool.execute("tc2", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(result.content, "dot root");
    }

    #[tokio::test]
    async fn include_metadata_prepends_json_line() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("sample.txt"), "hello\n").expect("write");

        let tool = ReadFileTool::new(
            ReadFileConfig {
                allowed_roots: vec![dir.path().to_path_buf()],
            },
            4096,
        );
        let ctx = ToolExecutionContext::new(
            ClientCwd::new(dir.path().to_path_buf()).expect("absolute client cwd"),
        );
        let args = json!({ "path": "sample.txt", "include_metadata": true });

        let (_record, result) = tool.execute("tc-meta", &args, 5000, &ctx).await;
        assert!(!result.is_error, "{}", result.content);
        assert!(result.content.starts_with(FILE_METADATA_PREFIX));
        let (meta, body) = result.content.split_once('\n').expect("metadata newline");
        assert!(meta.contains(&sha256_hex(b"hello\n")));
        assert_eq!(body, "hello");
    }
}
