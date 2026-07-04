//! write-like tool 実行前承認の stderr 表示と stdin 判定（`ai` 側 UI）。

use std::io::{IsTerminal, Write};

use aibe_client::ToolApprovalPrompt;
use aibe_protocol::ToolApprovalOrigin;

/// 承認プロンプト用に **制御文字のみ** escape し、UTF-8 テキスト（日本語等）はそのまま表示する。
pub fn escape_for_file_write_approval_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_control() {
            match ch {
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\n' => out.push_str("\\n"),
                _ => out.push_str(&format!("\\x{:02x}", ch as u32)),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// stderr へ出す承認プロンプト行（設計 §15.1）。
pub fn approval_prompt_stderr_lines(prompt: &ToolApprovalPrompt) -> Vec<String> {
    let mut lines = vec!["ai: file write approval required:".to_string()];
    lines.push(format!(
        "  tool: {}",
        escape_for_file_write_approval_display(&prompt.tool_name)
    ));
    if prompt.paths.is_empty() {
        lines.push("  path: (none)".to_string());
    } else if prompt.paths.len() == 1 {
        lines.push(format!(
            "  path: {}",
            escape_for_file_write_approval_display(&prompt.paths[0])
        ));
    } else {
        let escaped: Vec<String> = prompt
            .paths
            .iter()
            .map(|p| escape_for_file_write_approval_display(p))
            .collect();
        lines.push(format!("  paths: [{}]", escaped.join(", ")));
    }
    lines.push(format!(
        "  change: {}",
        escape_for_file_write_approval_display(&prompt.summary)
    ));
    if prompt.preview_truncated {
        lines.push("  note: preview truncated (see summary for full change stats)".to_string());
    }
    lines.push("  preview:".to_string());
    for line in prompt.preview.lines() {
        lines.push(escape_for_file_write_approval_display(line));
    }
    if prompt.preview_truncated && prompt.preview.is_empty() {
        lines.push("  (preview omitted due to size limit)".to_string());
    }
    lines
}

/// 対話的 stdin が使えるか（pipe / リダイレクトは false）。
pub fn stdin_ready_for_file_write_approval() -> bool {
    std::io::stdin().is_terminal()
}

pub fn parse_file_write_approval_choice(line: &str) -> Option<bool> {
    match line.trim() {
        "y" | "Y" | "yes" | "Yes" | "YES" => Some(true),
        "n" | "N" | "no" | "No" | "NO" | "" => Some(false),
        _ => None,
    }
}

fn denied_non_tty() -> ToolApprovalDecision {
    ToolApprovalDecision {
        approved: false,
        approval_origin: ToolApprovalOrigin::UiNo,
    }
}

/// テスト向け: stdin 準備状態と入力行から承認結果を決める。
pub fn file_write_approval_decision_from_input(
    stdin_ready: bool,
    choice_line: &str,
) -> ToolApprovalDecision {
    if !stdin_ready {
        return denied_non_tty();
    }
    match parse_file_write_approval_choice(choice_line) {
        Some(true) => ToolApprovalDecision {
            approved: true,
            approval_origin: ToolApprovalOrigin::UiYes,
        },
        Some(false) => ToolApprovalDecision {
            approved: false,
            approval_origin: ToolApprovalOrigin::UiNo,
        },
        None => ToolApprovalDecision {
            approved: false,
            approval_origin: ToolApprovalOrigin::UiNo,
        },
    }
}

/// write-like tool 承認の応答（`aibe-client` transport へ返す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolApprovalDecision {
    pub approved: bool,
    pub approval_origin: ToolApprovalOrigin,
}

/// ユーザーに yes/no を求め、承認なら decision を返す。
pub fn prompt_file_write_approval(prompt: ToolApprovalPrompt) -> ToolApprovalDecision {
    for line in approval_prompt_stderr_lines(&prompt) {
        eprintln!("{line}");
    }
    if !stdin_ready_for_file_write_approval() {
        eprintln!("ai: file write denied (non-interactive stdin)");
        return denied_non_tty();
    }
    eprint!("Apply this change? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let Ok(n) = std::io::stdin().read_line(&mut line) else {
        eprintln!("ai: file write denied (stdin unavailable)");
        return denied_non_tty();
    };
    if n == 0 {
        eprintln!("ai: file write denied (non-interactive stdin)");
        return denied_non_tty();
    }
    file_write_approval_decision_from_input(true, &line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::ToolRiskClass;

    fn sample_prompt() -> ToolApprovalPrompt {
        ToolApprovalPrompt {
            prompt_id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            tool_name: "apply_patch".into(),
            risk_class: ToolRiskClass::WriteLike,
            summary: "modify src/main.rs (+1 -1, 4 -> 4 bytes)".into(),
            paths: vec!["src/main.rs".into()],
            preview: "--- a/src/main.rs\n+++ b/src/main.rs\n@@\n-old\n+new\n".into(),
            preview_truncated: false,
        }
    }

    #[test]
    fn escape_control_chars_in_preview() {
        let raw = "\x1b[31mline\x1b[0m\n";
        let escaped = escape_for_file_write_approval_display(raw);
        assert!(escaped.contains("\\x1b"));
        assert!(escaped.contains("\\n"));
        assert!(!escaped.contains('\x1b'));
    }

    #[test]
    fn preserves_japanese_and_punctuation_in_preview() {
        let raw = "Line 5: ビルドの最適化と効率の改善が進行中です。\nLet's ensure";
        let escaped = escape_for_file_write_approval_display(raw);
        assert!(escaped.contains("ビルドの最適化"));
        assert!(escaped.contains("Let's ensure"));
        assert!(!escaped.contains("\\xe3"));
    }

    #[test]
    fn approval_lines_include_tool_path_and_preview() {
        let lines = approval_prompt_stderr_lines(&sample_prompt());
        let joined = lines.join("\n");
        assert!(joined.contains("tool: apply_patch"));
        assert!(joined.contains("path: src/main.rs"));
        assert!(joined.contains("change: modify src/main.rs"));
        assert!(joined.contains("preview:"));
        assert!(joined.contains("--- a/src/main.rs"));
    }

    #[test]
    fn truncation_notice_is_shown() {
        let mut prompt = sample_prompt();
        prompt.preview_truncated = true;
        let lines = approval_prompt_stderr_lines(&prompt);
        let joined = lines.join("\n");
        assert!(joined.contains("preview truncated"));
    }

    #[test]
    fn parse_choice_defaults_enter_to_no() {
        assert_eq!(parse_file_write_approval_choice("\n"), Some(false));
        assert_eq!(parse_file_write_approval_choice("y"), Some(true));
        assert!(parse_file_write_approval_choice("maybe").is_none());
    }

    #[test]
    fn non_tty_denies_before_read() {
        let decision = file_write_approval_decision_from_input(false, "y\n");
        assert!(!decision.approved);
        assert_eq!(decision.approval_origin, ToolApprovalOrigin::UiNo);
    }
}
