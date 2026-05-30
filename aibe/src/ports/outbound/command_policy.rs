//! `shell_exec` のコマンド許可ポリシー。

/// コマンド実行可否（MVP: 設定 allowlist。将来インタラクティブ許可を差し替え可能）。
pub trait CommandPolicy: Send + Sync {
    fn shell_exec_enabled(&self) -> bool;
    fn is_command_allowed(&self, command: &str) -> bool;
    fn shell_exec_approval_mode(&self) -> crate::ports::outbound::ShellExecApprovalMode;
}
