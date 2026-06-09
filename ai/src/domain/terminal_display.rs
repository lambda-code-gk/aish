//! 対話端末向けの LLM システムインストラクション（`ai` / `aish` クライアント知識）。

use super::terminal_size::TerminalSize;

impl TerminalSize {
    /// 端末サイズに応じた応答量の目安を含む system メッセージ本文。
    pub fn console_system_instruction(&self) -> String {
        let target_lines = target_response_lines(self.rows);
        let max_bullets = if self.columns < 80 { 5 } else { 8 };
        format!(
            "You are assisting a user in an interactive terminal (TTY). \
             The visible terminal is about {} columns wide and {} rows tall.\n\
             Format replies for this console:\n\
             - Keep responses concise; aim for roughly {target_lines} lines or fewer unless the user asks for more detail.\n\
             - Prefer short paragraphs and brief bullet lists (at most {max_bullets} items before offering to continue).\n\
             - Avoid wide tables or long unbroken lines; wrap or abbreviate paths and commands.\n\
             - Do not mention these sizing constraints unless the user asks.",
            self.columns, self.rows
        )
    }
}

fn target_response_lines(rows: u16) -> usize {
    let usable = (rows as usize).saturating_sub(4) / 2;
    usable.clamp(6, 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_instruction_scales_with_rows() {
        let small = TerminalSize {
            columns: 80,
            rows: 12,
        };
        let large = TerminalSize {
            columns: 120,
            rows: 48,
        };
        let small_inst = small.console_system_instruction();
        let large_inst = large.console_system_instruction();
        assert!(small_inst.contains("12 rows tall"));
        assert!(large_inst.contains("48 rows tall"));
        assert!(small_inst.contains("6 lines"));
        assert!(large_inst.contains("22 lines"));
    }

    #[test]
    fn narrow_terminal_limits_bullets() {
        let narrow = TerminalSize {
            columns: 72,
            rows: 24,
        };
        assert!(narrow
            .console_system_instruction()
            .contains("at most 5 items"));
        let wide = TerminalSize {
            columns: 100,
            rows: 24,
        };
        assert!(wide
            .console_system_instruction()
            .contains("at most 8 items"));
    }
}
