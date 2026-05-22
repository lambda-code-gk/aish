//! コマンド実行 outbound port。

use crate::domain::CommandSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("failed to run command: {0}")]
    Failed(String),
}

/// 子プロセスで 1 コマンドを実行する。
pub trait ShellExecutor {
    fn run(&self, command: &CommandSpec) -> Result<RunResult, ShellError>;
}
