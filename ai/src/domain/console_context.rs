//! 対話コンソール向けの `RequestContext` 補助。

use super::output_format::OutputFormat;
use super::request_context::RequestContextInput;
use super::terminal_size::TerminalSize;

impl RequestContextInput {
    /// TTY サイズから console 用 system インストラクションを付与する。
    ///
    /// `--format` で機械可読出力が指定された turn（`output_format` が `Some`）では、
    /// 端末幅に合わせた整形指示が出力の後段処理を歪めるため付与しない。
    pub fn with_console_system_instruction(
        mut self,
        terminal_size: Option<TerminalSize>,
        output_format: Option<OutputFormat>,
    ) -> Self {
        self.system_instruction = match output_format {
            Some(_) => None,
            None => terminal_size.map(|size| size.console_system_instruction()),
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
    fn attaches_instruction_on_tty_without_format() {
        let ctx = RequestContextInput::default()
            .with_console_system_instruction(Some(size()), None)
            .into_wire();
        assert!(ctx
            .system_instruction
            .as_deref()
            .is_some_and(|s| s.contains("120 columns")));
    }

    #[test]
    fn skips_instruction_when_format_is_specified() {
        for format in [OutputFormat::Tsv, OutputFormat::Json, OutputFormat::Env] {
            let ctx = RequestContextInput::default()
                .with_console_system_instruction(Some(size()), Some(format))
                .into_wire();
            assert!(ctx.system_instruction.is_none(), "format {format:?}");
        }
    }

    #[test]
    fn skips_instruction_without_terminal_size() {
        let ctx = RequestContextInput::default()
            .with_console_system_instruction(None, None)
            .into_wire();
        assert!(ctx.system_instruction.is_none());
    }
}
