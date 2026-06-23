//! 1 コマンド実行とログ追記ユースケース。

use crate::domain::{rfc3339_now, CommandKind, CommandSpec, LogEvent};
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
        const COMMAND_INDEX: u32 = 1;
        let started_at = rfc3339_now();
        self.log.append(&LogEvent::command_start_span(
            &command,
            COMMAND_INDEX,
            &started_at,
            CommandKind::Exec,
        ))?;

        let result = self.shell.run(&command)?;

        if !result.stdout.is_empty() {
            self.log
                .append(&LogEvent::stdout_indexed(&result.stdout, COMMAND_INDEX))?;
        }
        if !result.stderr.is_empty() {
            self.log
                .append(&LogEvent::stderr_indexed(&result.stderr, COMMAND_INDEX))?;
        }
        let finished_at = rfc3339_now();
        self.log.append(&LogEvent::command_end(
            COMMAND_INDEX,
            result.exit_code,
            &finished_at,
        ))?;

        Ok(ExecuteResult {
            exit_code: result.exit_code,
        })
    }
}
