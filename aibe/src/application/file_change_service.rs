//! prepare → approve → revalidate → journal → commit（設計 §17）。

use std::sync::Arc;

use aibe_protocol::{
    ExecutedToolCall, FileWriteApprovalOutcome, ToolApprovalOrigin, ToolRiskClass,
};
use async_trait::async_trait;

use crate::domain::{sha256_hex, FileChangeOperation, FileChangePlan};
use crate::ports::outbound::{
    FileChangeError, FileChangeExecuteRequest, FileChangeExecuteResult, FileChangeExecutor,
    FileChangeJournal, FileChangeJournalError, FileChangeStore, FileWriteApprovalMode,
    FileWriteConfig, JournalSaveRequest, ToolApprovalGateOutcome, ToolApprovalPromptRequest,
    ToolExecutionContext, TurnCancellation, TurnEventSink,
};

pub use crate::domain::{sanitize_apply_patch_arguments, sanitize_write_file_arguments};

/// ファイル変更オーケストレーション（設計 §17, §24.2）。
pub struct FileChangeService {
    config: FileWriteConfig,
    journal: Arc<dyn FileChangeJournal>,
    store: Arc<dyn FileChangeStore>,
}

impl FileChangeService {
    pub fn new(
        config: FileWriteConfig,
        journal: Arc<dyn FileChangeJournal>,
        store: Arc<dyn FileChangeStore>,
    ) -> Self {
        Self {
            config,
            journal,
            store,
        }
    }

    /// prepare 段階: plan を組み立てるだけでファイルは変更しない。
    pub fn prepare_plan(
        path: std::path::PathBuf,
        operation: FileChangeOperation,
        before: crate::domain::FileSnapshot,
        after_bytes: Vec<u8>,
        display_path: &str,
        max_preview_bytes: usize,
    ) -> FileChangePlan {
        crate::domain::prepare_file_change_plan(
            path,
            operation,
            before,
            after_bytes,
            display_path,
            max_preview_bytes,
        )
    }
}

#[async_trait]
impl FileChangeExecutor for FileChangeService {
    async fn execute(
        &self,
        request: FileChangeExecuteRequest,
        ctx: &ToolExecutionContext,
        cancellation: Option<&Arc<TurnCancellation>>,
        events: Option<&Arc<dyn TurnEventSink>>,
    ) -> Result<FileChangeExecuteResult, (FileChangeError, ExecutedToolCall)> {
        execute_inner(self, request, ctx, cancellation, events).await
    }
}

async fn execute_inner(
    service: &FileChangeService,
    request: FileChangeExecuteRequest,
    ctx: &ToolExecutionContext,
    cancellation: Option<&Arc<TurnCancellation>>,
    events: Option<&Arc<dyn TurnEventSink>>,
) -> Result<FileChangeExecuteResult, (FileChangeError, ExecutedToolCall)> {
    let approval_mode = service.config.approval;
    let approval_mode_str = approval_mode.as_str();

    if !service.config.enabled {
        let executed = error_executed(
            &request,
            approval_mode_str,
            FileWriteApprovalOutcome::PolicyNever,
            None,
            FileChangeError::ToolDisabled,
            "file write tools are disabled",
        );
        return Err((FileChangeError::ToolDisabled, executed));
    }

    if approval_mode == FileWriteApprovalMode::Never {
        let executed = error_executed(
            &request,
            approval_mode_str,
            FileWriteApprovalOutcome::PolicyNever,
            None,
            FileChangeError::WriteDeniedByPolicy,
            "file write denied by policy",
        );
        return Err((FileChangeError::WriteDeniedByPolicy, executed));
    }

    let approval_origin = if approval_mode == FileWriteApprovalMode::Always {
        None
    } else {
        let Some(gate) = ctx.tool_approval_gate() else {
            let executed = error_executed(
                &request,
                approval_mode_str,
                FileWriteApprovalOutcome::ApprovalUnavailable,
                None,
                FileChangeError::ApprovalUnavailable,
                "file write approval required but no interactive client connected",
            );
            return Err((FileChangeError::ApprovalUnavailable, executed));
        };

        if let Some(events) = events {
            events
                .progress(
                    ctx.turn_id(),
                    aibe_protocol::ProgressPhase::WaitingApproval,
                    Some(
                        request
                            .plan
                            .diff_preview
                            .summary
                            .display_line(request.plan.path.display().to_string().as_str()),
                    ),
                )
                .await;
        }

        let prompt = ToolApprovalPromptRequest {
            tool_name: request.tool_name.clone(),
            summary: request
                .plan
                .diff_preview
                .summary
                .display_line(request.plan.path.display().to_string().as_str()),
            paths: vec![request.plan.path.display().to_string()],
            preview: request.plan.diff_preview.diff_text.clone(),
            preview_truncated: request.plan.diff_preview.preview_truncated,
        };

        match gate
            .request_tool_approval(&request.tool_call_id, prompt)
            .await
        {
            ToolApprovalGateOutcome::Approved(origin) => Some(origin),
            ToolApprovalGateOutcome::Denied(origin) => {
                let executed = error_executed(
                    &request,
                    approval_mode_str,
                    FileWriteApprovalOutcome::UserDenied,
                    Some(origin),
                    FileChangeError::ApprovalDenied,
                    "file write rejected by user",
                );
                return Err((FileChangeError::ApprovalDenied, executed));
            }
            ToolApprovalGateOutcome::Unavailable => {
                let executed = error_executed(
                    &request,
                    approval_mode_str,
                    FileWriteApprovalOutcome::ApprovalUnavailable,
                    None,
                    FileChangeError::ApprovalUnavailable,
                    "file write approval required but no interactive client connected",
                );
                return Err((FileChangeError::ApprovalUnavailable, executed));
            }
            ToolApprovalGateOutcome::Cancelled => {
                let executed = error_executed(
                    &request,
                    approval_mode_str,
                    FileWriteApprovalOutcome::Cancelled,
                    None,
                    FileChangeError::Cancelled,
                    "file write cancelled",
                );
                return Err((FileChangeError::Cancelled, executed));
            }
            ToolApprovalGateOutcome::Timeout => {
                let executed = error_executed(
                    &request,
                    approval_mode_str,
                    FileWriteApprovalOutcome::Timeout,
                    None,
                    FileChangeError::Timeout,
                    "file write approval timed out",
                );
                return Err((FileChangeError::Timeout, executed));
            }
        }
    };

    if let Some(cancel) = cancellation {
        if cancel.is_cancelled() {
            let executed = error_executed(
                &request,
                approval_mode_str,
                FileWriteApprovalOutcome::Cancelled,
                None,
                FileChangeError::Cancelled,
                "file write cancelled",
            );
            return Err((FileChangeError::Cancelled, executed));
        }
    }

    if let Err(err) = revalidate(service, &request.plan).await {
        let executed = error_executed(
            &request,
            approval_mode_str,
            approval_outcome_for_mode(approval_mode, approval_origin),
            approval_origin,
            err,
            "file changed during approval wait",
        );
        return Err((err, executed));
    }

    let preserve_mode = request.plan.before.file_mode;
    let journal_result = service
        .journal
        .save_before(JournalSaveRequest {
            tool: request.tool_name.clone(),
            target_path: request.plan.path.clone(),
            before_state: request.plan.before.before_state,
            before_bytes: request.plan.before.bytes.clone(),
            before_sha256: request.plan.before_sha256.clone(),
            after_sha256: request.plan.after_sha256.clone(),
            after_bytes: request.plan.after_bytes.len(),
            file_mode: preserve_mode,
            operation: request.plan.operation,
            raw_patch: request.raw_patch.clone(),
        })
        .await;

    let entry = match journal_result {
        Ok(entry) => entry,
        Err(FileChangeJournalError::CapacityExceeded) => {
            let executed = error_executed(
                &request,
                approval_mode_str,
                approval_outcome_for_mode(approval_mode, approval_origin),
                approval_origin,
                FileChangeError::JournalCapacityExceeded,
                "journal capacity exceeded",
            );
            return Err((FileChangeError::JournalCapacityExceeded, executed));
        }
        Err(FileChangeJournalError::Failed) => {
            let executed = error_executed(
                &request,
                approval_mode_str,
                approval_outcome_for_mode(approval_mode, approval_origin),
                approval_origin,
                FileChangeError::JournalFailed,
                "journal save failed",
            );
            return Err((FileChangeError::JournalFailed, executed));
        }
    };

    if service
        .store
        .commit_atomic(&request.plan.path, &request.plan.after_bytes, preserve_mode)
        .await
        .is_err()
    {
        let executed = error_executed(
            &request,
            approval_mode_str,
            approval_outcome_for_mode(approval_mode, approval_origin),
            approval_origin,
            FileChangeError::WriteFailed,
            "atomic write failed",
        );
        return Err((FileChangeError::WriteFailed, executed));
    }

    let output = format!(
        "wrote {} bytes (change_id={})",
        request.plan.after_bytes.len(),
        entry.change_id
    );
    let mut executed = ExecutedToolCall::ok(
        request.tool_call_id.clone(),
        request.tool_name.clone(),
        request.sanitized_arguments.clone(),
        output,
    )
    .with_file_write_audit(
        approval_mode_str,
        approval_outcome_for_mode(approval_mode, approval_origin),
        approval_origin,
    );
    executed.risk_class = Some(ToolRiskClass::WriteLike);

    Ok(FileChangeExecuteResult {
        change_id: entry.change_id,
        executed,
    })
}

async fn revalidate(
    service: &FileChangeService,
    plan: &FileChangePlan,
) -> Result<(), FileChangeError> {
    let path = &plan.path;
    match plan.operation {
        FileChangeOperation::Create => {
            if service.store.path_exists(path).await {
                return Err(FileChangeError::StaleFile);
            }
        }
        FileChangeOperation::Replace | FileChangeOperation::Patch => {
            if !service.store.is_regular_file(path).await {
                return Err(FileChangeError::StaleFile);
            }
            let current = service
                .store
                .read_file_bytes(path)
                .await
                .map_err(|_| FileChangeError::StaleFile)?;
            let Some(current) = current else {
                return Err(FileChangeError::StaleFile);
            };
            let current_hash = sha256_hex(&current);
            if Some(current_hash) != plan.before_sha256 {
                return Err(FileChangeError::StaleFile);
            }
        }
    }
    Ok(())
}

fn error_executed(
    request: &FileChangeExecuteRequest,
    approval_mode: &str,
    outcome: FileWriteApprovalOutcome,
    origin: Option<ToolApprovalOrigin>,
    err: FileChangeError,
    message: &str,
) -> ExecutedToolCall {
    ExecutedToolCall::err(
        request.tool_call_id.clone(),
        request.tool_name.clone(),
        request.sanitized_arguments.clone(),
        err.error_code(),
        message,
    )
    .with_file_write_audit(approval_mode, outcome, origin)
}

fn approval_outcome_for_mode(
    mode: FileWriteApprovalMode,
    origin: Option<ToolApprovalOrigin>,
) -> FileWriteApprovalOutcome {
    match mode {
        FileWriteApprovalMode::Never => FileWriteApprovalOutcome::PolicyNever,
        FileWriteApprovalMode::Always => FileWriteApprovalOutcome::AutoApproved,
        FileWriteApprovalMode::Ask => {
            if origin == Some(ToolApprovalOrigin::UiYes) {
                FileWriteApprovalOutcome::UserApproved
            } else {
                FileWriteApprovalOutcome::UserDenied
            }
        }
    }
}
