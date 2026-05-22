//! 対話シェル outbound port。

#[derive(Debug, thiserror::Error)]
pub enum InteractiveShellError {
    #[error("interactive shell failed: {0}")]
    Failed(String),
}

/// PTY 上でログを取りながらシェルを起動する。
pub trait InteractiveShellRunner {
    fn run_shell(&mut self, shell: &str) -> Result<i32, InteractiveShellError>;
}

impl<S: InteractiveShellRunner + ?Sized> InteractiveShellRunner for &mut S {
    fn run_shell(&mut self, shell: &str) -> Result<i32, InteractiveShellError> {
        (*self).run_shell(shell)
    }
}
