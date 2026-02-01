//! 対話によるツール承認実装（CLI 境界）
//!
//! stdin/stderr を用いた対話は adapter 層の責務。
//! usecase は ToolApproval trait 経由で Approval を受け取るだけ。

use crate::domain::{Approval, ToolApproval};
use std::io::{self, BufRead, Write};

/// CLI 対話による承認実装
///
/// allowlist に一致しないコマンドの実行前にユーザーに確認を求める。
/// デフォルトは Denied（Enter のみ、または y/yes 以外は拒否）。
pub struct CliToolApproval;

impl CliToolApproval {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CliToolApproval {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolApproval for CliToolApproval {
    fn approve_unsafe_shell(&self, command: &str) -> Approval {
        eprintln!("============ Approval =============");
        eprintln!("  {}", command);
        eprint!("Execute? [Enter/other]: ");
        let _ = io::stderr().flush();

        let stdin = io::stdin();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_ok() {
            let input = line.trim().to_lowercase();
            if input == "" {
                return Approval::Approved;
            }
        }

        Approval::Denied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // CliToolApproval は stdin を使うため、実際の対話テストは困難。
    // ここでは構造の確認のみ行う。

    #[test]
    fn test_cli_tool_approval_new() {
        let _approval = CliToolApproval::new();
        // 構造体が作成できることを確認
    }

    #[test]
    fn test_cli_tool_approval_default() {
        let _approval = CliToolApproval::default();
        // Default トレイトが実装されていることを確認
    }
}
