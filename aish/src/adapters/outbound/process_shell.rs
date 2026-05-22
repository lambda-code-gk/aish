//! `std::process::Command` による shell outbound アダプタ。

use std::process::Command;

use crate::domain::CommandSpec;
use crate::ports::outbound::{RunResult, ShellError, ShellExecutor};

pub struct ProcessShell;

impl ShellExecutor for ProcessShell {
    fn run(&self, command: &CommandSpec) -> Result<RunResult, ShellError> {
        let output = Command::new(&command.program)
            .args(&command.args)
            .output()
            .map_err(|e| ShellError::Failed(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code();

        Ok(RunResult {
            stdout,
            stderr,
            exit_code,
        })
    }
}
