//! 対話コンソール向けの `RequestContext` 補助。

use super::request_context::RequestContextInput;
use super::terminal_size::TerminalSize;

impl RequestContextInput {
    /// 解決済み policy が有効なとき、TTY サイズから console 用 system インストラクションを付与する。
    pub fn with_console_system_instruction(
        mut self,
        terminal_size: Option<TerminalSize>,
        console_hints_effective: bool,
    ) -> Self {
        self.system_instruction = if console_hints_effective {
            terminal_size.map(|size| size.console_system_instruction())
        } else {
            None
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size() -> TerminalSize {
        TerminalSize {
            columns: 120,
            rows: 40,
        }
    }

    #[test]
    fn attaches_instruction_when_effective() {
        let ctx = RequestContextInput::default()
            .with_console_system_instruction(Some(size()), true)
            .into_wire();
        assert!(ctx
            .system_instruction
            .as_deref()
            .is_some_and(|s| s.contains("120 columns")));
    }

    #[test]
    fn skips_instruction_when_not_effective() {
        let ctx = RequestContextInput::default()
            .with_console_system_instruction(Some(size()), false)
            .into_wire();
        assert!(ctx.system_instruction.is_none());
    }

    #[test]
    fn skips_instruction_without_terminal_size() {
        let ctx = RequestContextInput::default()
            .with_console_system_instruction(None, true)
            .into_wire();
        assert!(ctx.system_instruction.is_none());
    }
}
