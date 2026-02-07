//! 承認 Outbound ポート（ツール実行時の安全確認）
//!
//! usecase はこの trait 経由で承認を取得し、対話の具体実装（stdin/stderr）は adapter 層に置く。

use common::error::Error;

/// 承認結果（Approved: 実行許可、Denied: 拒否）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// ユーザーが実行を許可した
    Approved,
    /// ユーザーが実行を拒否した
    Denied,
}

/// 危険操作の承認を得る Outbound ポート（adapter で実装）
///
/// usecase は stdin/stderr に直接触れず、このトレイト経由で承認を取得する。
/// 許可待ち中に Ctrl+C が押された場合は Err を返す。
pub trait ToolApproval: Send + Sync {
    /// allowlist に一致しないシェルコマンドの実行を承認するか確認する。
    /// 割り込み（Ctrl+C）の場合は Err を返す。
    fn approve_unsafe_shell(&self, command: &str) -> Result<Approval, Error>;
}
