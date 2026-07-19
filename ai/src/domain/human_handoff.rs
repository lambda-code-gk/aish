//! Minimal human handoff domain helpers（0055）。

use crate::domain::shell_single_quote;

pub const HANDOFF_ENV_KEYS: [&str; 5] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_PARENT_REQUEST",
    "AISH_HANDOFF_SUGGESTED_COMMAND",
    "AISH_HANDOFF_RUNTIME_DIR",
    "AISH_HANDOFF_TASK_JSON",
];

pub const HANDOFF_PARENT_REQUEST_MAX_BYTES: usize = 4 * 1024;

/// 親リクエスト表示用に bounded truncate する。
pub fn truncate_parent_request_summary(summary: &str) -> String {
    if summary.len() <= HANDOFF_PARENT_REQUEST_MAX_BYTES {
        return summary.to_string();
    }

    let mut start = summary
        .len()
        .saturating_sub(HANDOFF_PARENT_REQUEST_MAX_BYTES);

    while start < summary.len() && !summary.is_char_boundary(start) {
        start += 1;
    }

    format!("…{}", &summary[start..])
}

pub fn build_suggested_command(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        command.to_string()
    } else {
        let mut parts = vec![shell_single_quote(command)];
        parts.extend(args.iter().map(|arg| shell_single_quote(arg)));
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_command_preserves_shell_operators_in_args() {
        let text = build_suggested_command("git", &["grep".into(), "-n".into(), "foo|bar".into()]);
        assert!(text.contains("'foo|bar'"));
    }

    #[test]
    fn truncate_parent_request_ascii() {
        let input = "a".repeat(HANDOFF_PARENT_REQUEST_MAX_BYTES + 64);
        let out = truncate_parent_request_summary(&input);
        assert!(out.starts_with('…'));
        assert!(out.len() <= HANDOFF_PARENT_REQUEST_MAX_BYTES + 4);
        assert!(out.ends_with(&"a".repeat(64)));
    }

    #[test]
    fn truncate_parent_request_japanese() {
        let prefix = "あ".repeat(HANDOFF_PARENT_REQUEST_MAX_BYTES / 3 + 8);
        let out = truncate_parent_request_summary(&prefix);
        assert!(out.starts_with('…'));
        assert!(out.is_char_boundary(out.len()));
        assert!(out.chars().all(|c| c == '…' || c == 'あ'));
    }

    #[test]
    fn truncate_parent_request_mixed_utf8() {
        let mut input = String::new();
        while input.len() <= HANDOFF_PARENT_REQUEST_MAX_BYTES {
            input.push_str("ascii-日本語-");
        }
        input.push_str("tail-marker");
        let out = truncate_parent_request_summary(&input);
        assert!(out.starts_with('…'));
        assert!(out.ends_with("tail-marker"));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn truncate_parent_request_emoji() {
        let mut input = String::new();
        while input.len() <= HANDOFF_PARENT_REQUEST_MAX_BYTES {
            input.push('🙂');
        }
        input.push_str("end");
        let out = truncate_parent_request_summary(&input);
        assert!(out.starts_with('…'));
        assert!(out.ends_with("end"));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn truncate_parent_request_never_panics_at_boundary() {
        let units = ["a", "あ", "🙂"];
        for unit in units {
            let mut input = String::new();
            while input.len() <= HANDOFF_PARENT_REQUEST_MAX_BYTES + 8 {
                input.push_str(unit);
            }
            for trim in 0..=8 {
                if trim < input.len() {
                    let mut end = input.len() - trim;
                    while end > 0 && !input.is_char_boundary(end) {
                        end -= 1;
                    }
                    let slice = &input[..end];
                    let out = truncate_parent_request_summary(slice);
                    assert!(out.is_char_boundary(out.len()), "unit={unit} trim={trim}");
                }
            }
        }
        let mixed = format!("{}{}", "ascii-日本語-".repeat(512), "🙂".repeat(256));
        let out = truncate_parent_request_summary(&mixed);
        assert!(out.is_char_boundary(out.len()));
    }
}
