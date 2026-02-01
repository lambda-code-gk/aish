//! 承認 Port のドメイン型（ツール実行時の安全確認）
//!
//! usecase 層は ToolApproval trait を通じて承認を取得し、
//! 対話の具体実装（stdin/stderr）は adapter 層に置く。

/// 承認結果（Approved: 実行許可、Denied: 拒否）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    /// ユーザーが実行を許可した
    Approved,
    /// ユーザーが実行を拒否した
    Denied,
}

/// 危険操作の承認を得る Port（adapter で実装）
///
/// usecase は stdin/stderr に直接触れず、このトレイト経由で承認を取得する。
pub trait ToolApproval: Send + Sync {
    /// allowlist に一致しないシェルコマンドの実行を承認するか確認する
    ///
    /// # Arguments
    /// * `command` - 実行しようとしているコマンド文字列
    ///
    /// # Returns
    /// * `Approval::Approved` - ユーザーが許可した場合
    /// * `Approval::Denied` - ユーザーが拒否した場合（デフォルト）
    fn approve_unsafe_shell(&self, command: &str) -> Approval;
}

/// テスト用: 常に指定された結果を返す Stub
#[cfg(test)]
pub struct StubApproval {
    pub result: Approval,
}

#[cfg(test)]
impl StubApproval {
    pub fn approved() -> Self {
        Self { result: Approval::Approved }
    }

    pub fn denied() -> Self {
        Self { result: Approval::Denied }
    }
}

#[cfg(test)]
impl ToolApproval for StubApproval {
    fn approve_unsafe_shell(&self, _command: &str) -> Approval {
        self.result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_approval_approved() {
        let stub = StubApproval::approved();
        assert_eq!(stub.approve_unsafe_shell("rm -rf /"), Approval::Approved);
    }

    #[test]
    fn test_stub_approval_denied() {
        let stub = StubApproval::denied();
        assert_eq!(stub.approve_unsafe_shell("rm -rf /"), Approval::Denied);
    }
}
