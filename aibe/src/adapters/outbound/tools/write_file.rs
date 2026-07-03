//! `write_file` ツール（設計 §9）。

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::{Capability, ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{
    FileChangeError, FileChangeExecuteRequest, FileChangeExecutor, FileWriteConfig,
    ToolExecutionContext, ToolExecutor,
};

use super::file_change_common::{
    build_write_file_plan, load_before_snapshot, resolve_write_target, sanitized_write_file_args,
    validate_write_content, verify_expected_hash, WriteFileMode,
};
use super::safe_path::WritePathPolicy;

pub struct WriteFileTool {
    path_policy: WritePathPolicy,
    executor: Arc<dyn FileChangeExecutor>,
    config: FileWriteConfig,
}

impl WriteFileTool {
    pub fn new(config: FileWriteConfig, executor: Arc<dyn FileChangeExecutor>) -> Self {
        Self {
            path_policy: WritePathPolicy::from_config(&config),
            executor,
            config,
        }
    }

    fn tool_err(
        id: String,
        args: Value,
        code: &str,
        msg: impl Into<String>,
    ) -> (ExecutedToolCall, ToolResult) {
        let msg = msg.into();
        (
            ExecutedToolCall::err(id.clone(), ToolName::write_file(), args, code, &msg),
            ToolResult {
                tool_call_id: id,
                content: msg,
                is_error: true,
            },
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteFileArgs {
    path: String,
    mode: String,
    content: String,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[async_trait]
impl ToolExecutor for WriteFileTool {
    fn name(&self) -> ToolName {
        ToolName::write_file()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        _timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        let id = tool_call_id.to_string();
        let args_value = arguments.clone();

        if let Err(denied) = ctx.require_capability(Capability::FileWrite) {
            return Self::tool_err(id, args_value, "capability_denied", denied.message());
        }

        if !self.config.enabled {
            return Self::tool_err(
                id,
                args_value,
                "tool_disabled",
                "file write tools are disabled",
            );
        }

        let parsed: WriteFileArgs = match serde_json::from_value(arguments.clone()) {
            Ok(a) => a,
            Err(e) => {
                return Self::tool_err(
                    id,
                    args_value,
                    "invalid_arguments",
                    format!("invalid arguments: {e}"),
                );
            }
        };

        if parsed.path.trim().is_empty() {
            return Self::tool_err(
                id,
                args_value,
                "invalid_arguments",
                "path must not be empty",
            );
        }

        let mode = match WriteFileMode::parse(parsed.mode.as_str()) {
            Some(m) => m,
            None => {
                return Self::tool_err(
                    id,
                    args_value,
                    "invalid_arguments",
                    "mode must be create or replace",
                );
            }
        };

        if let Err((code, msg)) =
            validate_write_content(&parsed.content, self.config.max_file_bytes)
        {
            return Self::tool_err(id, args_value, code, msg);
        }

        let canonical = match resolve_write_target(&self.path_policy, ctx, &parsed.path).await {
            Ok(p) => p,
            Err((code, msg)) => return Self::tool_err(id, args_value, code, msg),
        };

        let before = match load_before_snapshot(&canonical, mode).await {
            Ok(s) => s,
            Err((code, msg)) => return Self::tool_err(id, args_value, code, msg),
        };

        if let Err((code, msg)) =
            verify_expected_hash(mode, parsed.expected_sha256.as_deref(), &before)
        {
            return Self::tool_err(id, args_value, code, msg);
        }

        let sanitized = sanitized_write_file_args(
            &parsed.path,
            mode,
            parsed.expected_sha256.as_deref(),
            &parsed.content,
        );

        let plan = build_write_file_plan(
            canonical,
            mode,
            before,
            &parsed.content,
            self.config.max_preview_bytes,
        );

        let request = FileChangeExecuteRequest {
            tool_call_id: id.clone(),
            tool_name: self.name().as_str().to_string(),
            plan,
            sanitized_arguments: sanitized,
            raw_patch: None,
        };

        match self.executor.execute(request, ctx, None, None).await {
            Ok(result) => {
                let content = result.executed.output.clone().unwrap_or_default();
                (
                    result.executed,
                    ToolResult {
                        tool_call_id: id,
                        content,
                        is_error: false,
                    },
                )
            }
            Err((err, executed)) => {
                let msg = executed.output.clone().unwrap_or_default();
                let code = match err {
                    FileChangeError::ToolDisabled => "tool_disabled",
                    FileChangeError::WriteDeniedByPolicy => "write_denied_by_policy",
                    FileChangeError::ApprovalDenied => "approval_denied",
                    FileChangeError::ApprovalUnavailable => "approval_unavailable",
                    FileChangeError::StaleFile => "stale_file",
                    FileChangeError::JournalFailed => "journal_failed",
                    FileChangeError::JournalCapacityExceeded => "journal_capacity_exceeded",
                    FileChangeError::WriteFailed => "write_failed",
                    FileChangeError::Cancelled => "cancelled",
                    FileChangeError::Timeout => "timeout",
                };
                (
                    executed,
                    ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        content: format!("[{code}] {msg}"),
                        is_error: true,
                    },
                )
            }
        }
    }
}
