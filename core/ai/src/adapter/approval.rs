//! 対話によるツール承認実装（CLI 境界）
//!
//! stdin/stderr を用いた対話は adapter 層の責務。
//! 許可待ち中は InterruptChecker をポーリングし、Ctrl+C で Err を返す。

use crate::domain::{Approval, ToolApproval};
use crate::ports::outbound::InterruptChecker;
use common::error::Error;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

/// CLI 対話による承認実装
///
/// allowlist に一致しないコマンドの実行前にユーザーに確認を求める。
/// Cursorに習い、Enter のみで承認とする。
/// interrupt_checker を渡すと、許可待ち中に Ctrl+C で割り込み可能。
pub struct CliToolApproval {
    interrupt_checker: Option<Arc<dyn InterruptChecker>>,
}

impl CliToolApproval {
    pub fn new(interrupt_checker: Option<Arc<dyn InterruptChecker>>) -> Self {
        Self {
            interrupt_checker,
        }
    }
}

impl Default for CliToolApproval {
    fn default() -> Self {
        Self::new(None)
    }
}

/// 非対話用: 常に拒否を返す（CI 等でプロンプトを出さない）
pub struct NonInteractiveToolApproval;

impl NonInteractiveToolApproval {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NonInteractiveToolApproval {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolApproval for NonInteractiveToolApproval {
    fn approve_unsafe_shell(&self, _command: &str) -> Result<Approval, Error> {
        Ok(Approval::Denied)
    }
}

impl ToolApproval for CliToolApproval {
    fn approve_unsafe_shell(&self, command: &str) -> Result<Approval, Error> {
        eprintln!("============ Approval =============");
        eprintln!("  {}", command);
        eprint!("Execute? [Enter/No(other)]: ");
        let _ = io::stderr().flush();

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let stdin = io::stdin();
            let mut line = String::new();
            let approval = if stdin.lock().read_line(&mut line).is_ok() {
                let input = line.trim().to_lowercase();
                if input.is_empty() {
                    Approval::Approved
                } else {
                    Approval::Denied
                }
            } else {
                Approval::Denied
            };
            let _ = tx.send(approval);
        });

        let timeout = Duration::from_millis(100);
        loop {
            match rx.recv_timeout(timeout) {
                Ok(approval) => return Ok(approval),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self
                        .interrupt_checker
                        .as_ref()
                        .map_or(false, |c| c.is_interrupted())
                    {
                        return Err(Error::system(
                            "Interrupted by user (Ctrl+C) during approval prompt.",
                        ));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(Approval::Denied),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CliToolApproval は stdin を使うため、実際の対話テストは困難。
    // ここでは構造の確認のみ行う。

    #[test]
    fn test_cli_tool_approval_new() {
        let _approval = CliToolApproval::new(None);
        // 構造体が作成できることを確認
    }

    #[test]
    fn test_cli_tool_approval_default() {
        let _approval = CliToolApproval::default();
        // Default トレイトが実装されていることを確認
    }

    #[test]
    fn test_non_interactive_tool_approval_always_denied() {
        let approval = NonInteractiveToolApproval::new();
        assert_eq!(
            approval.approve_unsafe_shell("rm -rf /").unwrap(),
            Approval::Denied
        );
    }
}
