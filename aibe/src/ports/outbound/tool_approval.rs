//! write-like tool 実行前承認（同一 socket 接続上の往復）。

use async_trait::async_trait;

use aibe_protocol::ToolApprovalOrigin;

/// 承認待ち timeout（設計 §14.6 — `exec_timeout_ms` とは独立）。
pub const FILE_WRITE_APPROVAL_TIMEOUT_MS: u64 = 10 * 60 * 1000;

/// 承認 UI へ送る prompt 本文（wire DTO への変換前）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolApprovalPromptRequest {
    pub tool_name: String,
    pub summary: String,
    pub paths: Vec<String>,
    pub preview: String,
    pub preview_truncated: bool,
}

/// 承認待ちの結果（設計 §14.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalGateOutcome {
    Approved(ToolApprovalOrigin),
    Denied(ToolApprovalOrigin),
    Unavailable,
    Cancelled,
    Timeout,
}

/// 実行直前の yes/no 応答。`ask` モードでのみ使用する。
#[async_trait]
pub trait ToolApprovalGate: Send + Sync {
    async fn request_tool_approval(
        &self,
        tool_call_id: &str,
        prompt: ToolApprovalPromptRequest,
    ) -> ToolApprovalGateOutcome;
}
