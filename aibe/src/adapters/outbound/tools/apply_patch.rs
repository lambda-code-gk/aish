//! `apply_patch` ツール（設計 §10）。

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::{
    sanitize_apply_patch_arguments_best_effort, Capability, ExecutedToolCall, ToolName, ToolResult,
};
use crate::ports::outbound::{
    FileChangeError, FileChangeExecuteRequest, FileChangeExecutor, FileWriteConfig,
    ToolExecutionContext, ToolExecutor,
};
use aibe_protocol::ToolRiskClass;

use super::file_change_common::{
    build_patch_plan, load_patch_target_snapshot, resolve_write_target, sanitized_apply_patch_args,
    verify_patch_expected_hash,
};
use super::patch_parser::{
    apply_hunks_to_lines, encode_file_lines, parse_unified_hunks, split_file_lines, PatchError,
};
use super::safe_path::WritePathPolicy;

pub struct ApplyPatchTool {
    path_policy: WritePathPolicy,
    executor: Arc<dyn FileChangeExecutor>,
    config: FileWriteConfig,
}

impl ApplyPatchTool {
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
            ExecutedToolCall::err(id.clone(), ToolName::apply_patch(), args, code, &msg),
            ToolResult {
                tool_call_id: id,
                content: msg,
                is_error: true,
            },
        )
    }

    fn map_patch_error(err: PatchError) -> (&'static str, String) {
        match err {
            PatchError::InvalidPatch => ("invalid_patch", "invalid patch".into()),
            PatchError::PatchConflict => ("patch_conflict", "patch context mismatch".into()),
            PatchError::OverlappingHunks => ("invalid_patch", "overlapping hunks".into()),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyPatchArgs {
    path: String,
    patch: String,
    expected_sha256: String,
}

#[async_trait]
impl ToolExecutor for ApplyPatchTool {
    fn name(&self) -> ToolName {
        ToolName::apply_patch()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        _timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        let id = tool_call_id.to_string();
        let audit_args = sanitize_apply_patch_arguments_best_effort(arguments);

        if let Err(denied) = ctx.require_capability(Capability::FileWrite) {
            return Self::tool_err(id, audit_args, "capability_denied", denied.message());
        }

        if !self.config.enabled {
            return Self::tool_err(
                id,
                audit_args,
                "tool_disabled",
                "file write tools are disabled",
            );
        }

        let parsed: ApplyPatchArgs = match serde_json::from_value(arguments.clone()) {
            Ok(a) => a,
            Err(e) => {
                return Self::tool_err(
                    id,
                    audit_args,
                    "invalid_arguments",
                    format!("invalid arguments: {e}"),
                );
            }
        };

        if parsed.path.trim().is_empty() {
            return Self::tool_err(
                id,
                audit_args,
                "invalid_arguments",
                "path must not be empty",
            );
        }

        if parsed.patch.is_empty() {
            return Self::tool_err(id, audit_args, "invalid_patch", "patch must not be empty");
        }

        if parsed.patch.len() > self.config.max_patch_bytes {
            return Self::tool_err(id, audit_args, "input_too_large", "patch too large");
        }

        let canonical = match resolve_write_target(&self.path_policy, ctx, &parsed.path).await {
            Ok(p) => p,
            Err((code, msg)) => return Self::tool_err(id, audit_args, code, msg),
        };

        let (before, line_ending) =
            match load_patch_target_snapshot(&canonical, self.config.max_file_bytes).await {
                Ok(s) => s,
                Err((code, msg)) => return Self::tool_err(id, audit_args, code, msg),
            };

        if let Err((code, msg)) = verify_patch_expected_hash(&parsed.expected_sha256, &before) {
            return Self::tool_err(id, audit_args, code, msg);
        }

        let hunks = match parse_unified_hunks(&parsed.patch) {
            Ok(h) => h,
            Err(err) => {
                let (code, msg) = Self::map_patch_error(err);
                return Self::tool_err(id, audit_args, code, msg);
            }
        };

        let before_bytes = before.bytes.as_deref().unwrap_or_default();
        let file_lines = split_file_lines(before_bytes, line_ending);
        let applied = match apply_hunks_to_lines(&file_lines, &hunks) {
            Ok(applied) => applied,
            Err(err) => {
                let (code, msg) = Self::map_patch_error(err);
                return Self::tool_err(id, audit_args, code, msg);
            }
        };

        let after_bytes = encode_file_lines(&applied.lines, line_ending, applied.trailing_newline);
        if after_bytes.len() > self.config.max_file_bytes {
            return Self::tool_err(id, audit_args, "input_too_large", "result file too large");
        }

        let sanitized = sanitized_apply_patch_args(
            &parsed.path,
            &parsed.expected_sha256,
            &parsed.patch,
            hunks.len(),
        );

        if after_bytes == before_bytes {
            let mut executed = ExecutedToolCall::ok(
                id.clone(),
                ToolName::apply_patch().as_str(),
                sanitized,
                "no change".into(),
            );
            executed.risk_class = Some(ToolRiskClass::WriteLike);
            executed.decision = Some("no_change".into());
            executed.dry_run = Some(false);
            return (
                executed,
                ToolResult {
                    tool_call_id: id,
                    content: "no change".into(),
                    is_error: false,
                },
            );
        }

        let display = canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| canonical.display().to_string());
        let plan = build_patch_plan(
            canonical,
            before,
            after_bytes,
            &display,
            self.config.max_preview_bytes,
        );

        let request = FileChangeExecuteRequest {
            tool_call_id: id.clone(),
            tool_name: self.name().as_str().to_string(),
            plan,
            sanitized_arguments: sanitized,
            raw_patch: Some(parsed.patch),
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

#[cfg(test)]
mod tests {
    use aibe_protocol::ExecutedToolStatus;

    use super::*;

    #[test]
    fn no_change_decision_is_set() {
        let mut executed =
            ExecutedToolCall::ok("id".into(), "apply_patch", Value::Null, "no change".into());
        executed.decision = Some("no_change".into());
        assert_eq!(executed.decision.as_deref(), Some("no_change"));
        assert_eq!(executed.status, ExecutedToolStatus::Ok);
    }
}
