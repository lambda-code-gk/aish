//! 標準サブプロセス実行（std::process::Command を委譲）

use crate::ports::outbound::Process;
use crate::error::Error;
use std::path::Path;

/// 標準ライブラリの Command を使う Process 実装
#[derive(Debug, Clone, Default)]
pub struct StdProcess;

impl Process for StdProcess {
    fn run(&self, program: &Path, args: &[String]) -> Result<i32, Error> {
        let status = std::process::Command::new(program)
            .args(args)
            .status()
            .map_err(|e| {
                Error::io_msg(format!(
                    "Failed to execute '{}': {}",
                    program.display(),
                    e
                ))
            })?;
        Ok(status.code().unwrap_or(1))
    }
}
