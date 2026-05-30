//! `shell_exec` 実行前承認（同一 socket 接続上の往復）。

use async_trait::async_trait;

/// 実行直前の yes/no 応答。`ask` モードでのみ使用する。
#[async_trait]
pub trait ShellExecApprovalGate: Send + Sync {
    async fn request_shell_exec_approval(
        &self,
        tool_call_id: &str,
        command: &str,
        args: &[String],
    ) -> bool;
}
