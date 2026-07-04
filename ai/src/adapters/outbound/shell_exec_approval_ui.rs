//! `shell_exec` 実行前承認の stderr 表示と stdin 判定（`ai` 側 UI）。

use std::io::{IsTerminal, Write};

use aibe_client::ShellExecApprovalPrompt;
use aibe_protocol::ShellExecApprovalOrigin;

use crate::domain::{
    shell_exec_approval_origin_for_choice, ShellExecApprovalChoice, ShellExecApprovalDecision,
    ShellExecTier,
};

/// 承認プロンプト用に制御文字を escape して 1 行文字列にする。
pub fn escape_for_shell_exec_approval_display(s: &str) -> String {
    let escaped: Vec<u8> = s.bytes().flat_map(std::ascii::escape_default).collect();
    String::from_utf8_lossy(&escaped).into_owned()
}

/// `command` と `args` を 1 行の invocation 文字列にする。
pub fn format_shell_exec_invocation(command: &str, args: &[String]) -> String {
    let cmd = escape_for_shell_exec_approval_display(command);
    if args.is_empty() {
        cmd
    } else {
        let escaped: Vec<String> = args
            .iter()
            .map(|a| escape_for_shell_exec_approval_display(a))
            .collect();
        format!("{cmd} {}", escaped.join(" "))
    }
}

fn shell_exec_approval_origin_label(origin: ShellExecApprovalOrigin) -> &'static str {
    match origin {
        ShellExecApprovalOrigin::SessionAllowed => "session/read_only",
        ShellExecApprovalOrigin::SessionCacheExactInvocation => "session-cache/exact",
        ShellExecApprovalOrigin::SessionCacheCommandName => "session-cache/command",
        ShellExecApprovalOrigin::PatternReadOnly => "pattern/read_only",
        ShellExecApprovalOrigin::PatternMutating => "pattern/mutating",
        ShellExecApprovalOrigin::CollaborativeHandoff => "collaborative/handoff",
        _ => "auto",
    }
}

/// 自動承認時に stderr へ出す 1 行。
pub fn format_auto_approved_shell_exec_line(
    prompt: &ShellExecApprovalPrompt,
    tier: ShellExecTier,
    origin: ShellExecApprovalOrigin,
) -> String {
    format!(
        "ai: shell_exec auto-approved ({}, tier={}): {}",
        shell_exec_approval_origin_label(origin),
        tier.as_str(),
        format_shell_exec_invocation(&prompt.command, &prompt.args),
    )
}

/// 自動承認時に stderr へ通知する（`silent` なら何も出さない）。
pub fn emit_auto_approved_shell_exec(
    prompt: &ShellExecApprovalPrompt,
    tier: ShellExecTier,
    origin: ShellExecApprovalOrigin,
    silent: bool,
) {
    if silent {
        return;
    }
    eprintln!(
        "{}",
        format_auto_approved_shell_exec_line(prompt, tier, origin)
    );
}

pub fn approval_prompt_stderr_lines(
    prompt: &ShellExecApprovalPrompt,
    tier: ShellExecTier,
    session_shell_allowed: bool,
) -> Vec<String> {
    let mut lines = vec!["ai: shell_exec approval required:".to_string()];
    lines.push(format!("  tier: {}", tier.as_str()));
    lines.push(format!("  session_shell_allowed: {session_shell_allowed}"));
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
    lines.push("  choices: [y]es / [n]o / [a]lways-same-args / [c]ommand-only".to_string());
    lines.push(
        "  hint: [a] remembers this exact command+args; [c] remembers command name within tier"
            .to_string(),
    );
    lines
}

/// 対話的 stdin が使えるか（pipe / リダイレクトは false）。
pub fn stdin_ready_for_shell_exec_approval() -> bool {
    std::io::stdin().is_terminal()
}

pub fn parse_shell_exec_choice(line: &str) -> Option<ShellExecApprovalChoice> {
    match line.trim() {
        "y" | "Y" | "yes" | "Yes" | "YES" => Some(ShellExecApprovalChoice::Yes),
        "n" | "N" | "no" | "No" | "NO" => Some(ShellExecApprovalChoice::No),
        "a" | "A" => Some(ShellExecApprovalChoice::AlwaysThisSession),
        "c" | "C" => Some(ShellExecApprovalChoice::CommandOnly),
        _ => None,
    }
}

/// ユーザーに yes/no/a/c を求め、承認なら decision を返す。
pub fn prompt_shell_exec_approval(
    prompt: ShellExecApprovalPrompt,
    tier: ShellExecTier,
    session_shell_allowed: bool,
) -> ShellExecApprovalDecision {
    for line in approval_prompt_stderr_lines(&prompt, tier, session_shell_allowed) {
        eprintln!("{line}");
    }
    if !stdin_ready_for_shell_exec_approval() {
        eprintln!("ai: shell_exec denied (non-interactive stdin)");
        return ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::UiNo,
            remember_scope: None,
        };
    }
    eprint!("Execute? [y/n/a/c] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let Ok(n) = std::io::stdin().read_line(&mut line) else {
        eprintln!("ai: shell_exec denied (stdin unavailable)");
        return ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::UiNo,
            remember_scope: None,
        };
    };
    if n == 0 {
        eprintln!("ai: shell_exec denied (non-interactive stdin)");
        return ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::UiNo,
            remember_scope: None,
        };
    }

    match parse_shell_exec_choice(&line) {
        Some(ShellExecApprovalChoice::Yes) => ShellExecApprovalDecision {
            approved: true,
            approval_origin: shell_exec_approval_origin_for_choice(ShellExecApprovalChoice::Yes),
            remember_scope: None,
        },
        Some(ShellExecApprovalChoice::No) => ShellExecApprovalDecision {
            approved: false,
            approval_origin: shell_exec_approval_origin_for_choice(ShellExecApprovalChoice::No),
            remember_scope: None,
        },
        Some(ShellExecApprovalChoice::AlwaysThisSession) => ShellExecApprovalDecision {
            approved: true,
            approval_origin: shell_exec_approval_origin_for_choice(
                ShellExecApprovalChoice::AlwaysThisSession,
            ),
            remember_scope: Some(crate::domain::ShellExecRememberScope::ExactInvocation),
        },
        Some(ShellExecApprovalChoice::CommandOnly) => ShellExecApprovalDecision {
            approved: true,
            approval_origin: shell_exec_approval_origin_for_choice(
                ShellExecApprovalChoice::CommandOnly,
            ),
            remember_scope: if tier == ShellExecTier::Destructive {
                None
            } else {
                Some(crate::domain::ShellExecRememberScope::CommandName)
            },
        },
        None => ShellExecApprovalDecision {
            approved: false,
            approval_origin: ShellExecApprovalOrigin::UiNo,
            remember_scope: None,
        },
    }
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
    fn approval_lines_show_choices_and_tier() {
        let prompt = ShellExecApprovalPrompt {
            prompt_id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            command: "\x1b[31mecho\x1b[0m".into(),
            args: vec!["a\nb".into()],
        };
        let lines = approval_prompt_stderr_lines(&prompt, ShellExecTier::Mutating, true);
        let joined = lines.join("\n");
        assert!(joined.contains("choices: [y]es / [n]o / [a]lways-same-args / [c]ommand-only"));
        assert!(joined.contains("hint: [a] remembers this exact command+args"));
        assert!(joined.contains("tier: mutating"));
        assert!(joined.contains("\\x1b"));
        assert!(joined.contains("\\n"));
    }

    #[test]
    fn parse_choice_variants() {
        assert!(matches!(
            parse_shell_exec_choice("y\n"),
            Some(ShellExecApprovalChoice::Yes)
        ));
        assert!(matches!(
            parse_shell_exec_choice("c"),
            Some(ShellExecApprovalChoice::CommandOnly)
        ));
        assert!(parse_shell_exec_choice("wat").is_none());
    }

    #[test]
    fn auto_approved_line_shows_invocation_and_origin() {
        let prompt = ShellExecApprovalPrompt {
            prompt_id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            command: "git".into(),
            args: vec!["status".into()],
        };
        let line = format_auto_approved_shell_exec_line(
            &prompt,
            ShellExecTier::ReadOnly,
            ShellExecApprovalOrigin::PatternReadOnly,
        );
        assert!(line.contains("auto-approved (pattern/read_only, tier=read_only): git status"));
    }

    #[test]
    fn emit_auto_approved_respects_silent() {
        let prompt = ShellExecApprovalPrompt {
            prompt_id: "p1".into(),
            turn_id: "t1".into(),
            tool_call_id: "c1".into(),
            command: "echo".into(),
            args: vec!["hi".into()],
        };
        emit_auto_approved_shell_exec(
            &prompt,
            ShellExecTier::ReadOnly,
            ShellExecApprovalOrigin::SessionAllowed,
            true,
        );
    }
}
