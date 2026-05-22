//! 1 コマンド実行とログ追記ユースケース。

use crate::domain::{sanitize_log_text, CommandSpec, LogEvent};
use crate::ports::outbound::{LogError, SessionLog, ShellError, ShellExecutor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteResult {
    pub exit_code: Option<i32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecuteError {
    #[error(transparent)]
    Shell(#[from] ShellError),
    #[error(transparent)]
    Log(#[from] LogError),
}

pub struct ExecuteAndRecord<S, L> {
    shell: S,
    log: L,
}

impl<S, L> ExecuteAndRecord<S, L>
where
    S: ShellExecutor,
    L: SessionLog,
{
    pub fn new(shell: S, log: L) -> Self {
        Self { shell, log }
    }

    pub fn run(&mut self, command: CommandSpec) -> Result<ExecuteResult, ExecuteError> {
        self.log.append(&LogEvent::command_start(&command))?;

        let result = self.shell.run(&command)?;

        if !result.stdout.is_empty() {
            self.log.append(&LogEvent::Stdout {
                data: sanitize_log_text(&result.stdout),
            })?;
        }
        if !result.stderr.is_empty() {
            self.log.append(&LogEvent::Stderr {
                data: sanitize_log_text(&result.stderr),
            })?;
        }
        self.log.append(&LogEvent::Exit {
            code: result.exit_code,
        })?;

        Ok(ExecuteResult {
            exit_code: result.exit_code,
        })
    }
}
