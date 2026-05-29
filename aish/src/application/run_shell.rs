//! 対話シェルユースケース（ログの `CommandStart` / `Exit` は呼び出し側で追記可）。

use std::path::Path;

use crate::ports::outbound::{InteractiveShellError, InteractiveShellRunner};

pub struct RunShell<S> {
    shell: S,
}

impl<S> RunShell<S>
where
    S: InteractiveShellRunner,
{
    pub fn new(shell: S) -> Self {
        Self { shell }
    }

    pub fn run(&mut self, shell: &str, session_dir: &Path) -> Result<i32, InteractiveShellError> {
        self.shell.run_shell(shell, session_dir)
    }
}
