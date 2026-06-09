//! 対話コンソール向けの `RequestContext` 補助。

use super::request_context::RequestContextInput;
use super::terminal_size::TerminalSize;

impl RequestContextInput {
    /// TTY サイズから console 用 system インストラクションを付与する。
    pub fn with_console_system_instruction(mut self, terminal_size: Option<TerminalSize>) -> Self {
        self.system_instruction = terminal_size.map(|size| size.console_system_instruction());
        self
    }
}
