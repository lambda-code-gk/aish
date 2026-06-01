//! `shell_exec` 実行前承認の stderr 表示と stdin 判定（`ai` 側 UI）。

use std::io::{IsTerminal, Write};

use aibe_client::ShellExecApprovalPrompt;

/// 承認プロンプト用に制御文字を escape して 1 行文字列にする。
pub fn escape_for_shell_exec_approval_display(s: &str) -> String {
    let escaped: Vec<u8> = s.bytes().flat_map(std::ascii::escape_default).collect();
    String::from_utf8_lossy(&escaped).into_owned()
}

/// 承認プロンプトを stderr に出す行一覧（テスト用）。
pub fn approval_prompt_stderr_lines(prompt: &ShellExecApprovalPrompt) -> Vec<String> {
    let mut lines = vec!["ai: shell_exec approval required:".to_string()];
    lines.push(format!(
        "  command: {}",
        escape_for_shell_exec_approval_display(&prompt.command)
    ));
    if prompt.args.is_empty() {
        lines.push("  args: (none)".to_string());
    } else {
        let escaped: Vec<String> = prompt
            .args
            .iter()
            .map(|a| escape_for_shell_exec_approval_display(a))
            .collect();
        lines.push(format!("  args: [{}]", escaped.join(", ")));
    }
    lines
}

/// 対話的 stdin が使えるか（pipe / リダイレクトは false）。
pub fn stdin_ready_for_shell_exec_approval() -> bool {
    std::io::stdin().is_terminal()
}

pub fn parse_approval_yes(line: &str) -> bool {
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

/// ユーザーに yes/no を求め、承認なら true。
pub fn prompt_shell_exec_approval(prompt: ShellExecApprovalPrompt) -> bool {
    for line in approval_prompt_stderr_lines(&prompt) {
        eprintln!("{line}");
    }
    if !stdin_ready_for_shell_exec_approval() {
        eprintln!("ai: shell_exec denied (non-interactive stdin)");
        return false;
    }
    eprint!("Execute? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let Ok(n) = std::io::stdin().read_line(&mut line) else {
        eprintln!("ai: shell_exec denied (stdin unavailable)");
        return false;
    };
    if n == 0 {
        eprintln!("ai: shell_exec denied (non-interactive stdin)");
        return false;
    }
    parse_approval_yes(&line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_ansi_and_newline() {
        let raw = "\x1b[31mls\x1b[0m\n";
        let escaped = escape_for_shell_exec_approval_display(raw);
        assert!(
            escaped.contains("\\x1b"),
            "expected \\x1b escape, got {escaped:?}"
        );
        assert!(
            escaped.contains("\\n"),
            "expected \\n escape, got {escaped:?}"
        );
        assert!(!escaped.contains('\x1b'), "raw ESC must not appear");
        assert!(!escaped.contains('\n'), "raw newline must not appear");
    }

    #[test]
    fn approval_lines_use_escape() {
        let prompt = ShellExecApprovalPrompt {
            prompt_id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            command: "\x1b[31mecho\x1b[0m".into(),
            args: vec!["a\nb".into()],
        };
        let lines = approval_prompt_stderr_lines(&prompt);
        let joined = lines.join("\n");
        assert!(joined.contains("\\x1b"));
        assert!(joined.contains("\\n"));
        assert!(!joined.contains('\x1b'));
    }

    #[test]
    fn parse_approval_yes_variants() {
        assert!(parse_approval_yes("y\n"));
        assert!(parse_approval_yes("YES"));
        assert!(!parse_approval_yes("n"));
        assert!(!parse_approval_yes(""));
    }

    #[test]
    fn stdin_ready_matches_is_terminal() {
        assert_eq!(
            stdin_ready_for_shell_exec_approval(),
            std::io::stdin().is_terminal()
        );
    }
}
