//! ファイル変更オーケストレーション port（設計 §17）。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::domain::{ExecutedToolCall, FileChangePlan};
use crate::ports::outbound::ToolExecutionContext;

/// 実行要求（prepare 済み plan + 監査用引数）。
#[derive(Debug, Clone)]
pub struct FileChangeExecuteRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub plan: FileChangePlan,
    pub sanitized_arguments: Value,
    pub raw_patch: Option<String>,
}

/// 成功時の commit 結果。
#[derive(Debug, Clone)]
pub struct FileChangeExecuteResult {
    pub change_id: String,
    pub executed: ExecutedToolCall,
}

/// 実行失敗（設計 §21）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeError {
    ToolDisabled,
    WriteDeniedByPolicy,
    ApprovalDenied,
    ApprovalUnavailable,
    StaleFile,
    JournalFailed,
    JournalCapacityExceeded,
    WriteFailed,
    Cancelled,
    Timeout,
}

impl FileChangeError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::ToolDisabled => "tool_disabled",
            Self::WriteDeniedByPolicy => "write_denied_by_policy",
            Self::ApprovalDenied => "approval_denied",
            Self::ApprovalUnavailable => "approval_unavailable",
            Self::StaleFile => "stale_file",
            Self::JournalFailed => "journal_failed",
            Self::JournalCapacityExceeded => "journal_capacity_exceeded",
            Self::WriteFailed => "write_failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
        }
    }
}

/// prepare → approve → revalidate → journal → commit（設計 §17）。
#[async_trait]
pub trait FileChangeExecutor: Send + Sync {
    async fn execute(
        &self,
        request: FileChangeExecuteRequest,
        ctx: &ToolExecutionContext,
        cancellation: Option<&Arc<crate::ports::outbound::TurnCancellation>>,
        events: Option<&Arc<dyn crate::ports::outbound::TurnEventSink>>,
    ) -> Result<FileChangeExecuteResult, (FileChangeError, ExecutedToolCall)>;
}
