//! 対話シェル outbound port。

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum InteractiveShellError {
    #[error("interactive shell failed: {0}")]
    Failed(String),
}

/// PTY 上でログを取りながらシェルを起動する。
pub trait InteractiveShellRunner {
    fn run_shell(&mut self, shell: &str, session_dir: &Path) -> Result<i32, InteractiveShellError>;
}

impl<S: InteractiveShellRunner + ?Sized> InteractiveShellRunner for &mut S {
    fn run_shell(&mut self, shell: &str, session_dir: &Path) -> Result<i32, InteractiveShellError> {
        (*self).run_shell(shell, session_dir)
    }
}
