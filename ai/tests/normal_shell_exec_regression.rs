//! 通常 shell_exec 経路の回帰（0055 minimal）。

use ai::domain::{classify_shell_exec_tier, ShellExecTier};

#[test]
fn normal_shell_exec_tier_classification_unchanged() {
    assert_eq!(
        classify_shell_exec_tier("git", &["status".into()]),
        ShellExecTier::ReadOnly
    );
    assert_eq!(
        classify_shell_exec_tier("rm", &["-rf".into(), "/".into()]),
        ShellExecTier::Destructive
    );
}
