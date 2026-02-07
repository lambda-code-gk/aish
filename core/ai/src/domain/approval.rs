//! 承認 Port の re-export とテスト用 Stub（定義は ports/outbound/approval）

pub use crate::ports::outbound::{Approval, ToolApproval};

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
    fn approve_unsafe_shell(&self, _command: &str) -> Result<Approval, common::error::Error> {
        Ok(self.result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_approval_approved() {
        let stub = StubApproval::approved();
        assert_eq!(stub.approve_unsafe_shell("rm -rf /").unwrap(), Approval::Approved);
    }

    #[test]
    fn test_stub_approval_denied() {
        let stub = StubApproval::denied();
        assert_eq!(stub.approve_unsafe_shell("rm -rf /").unwrap(), Approval::Denied);
    }
}
