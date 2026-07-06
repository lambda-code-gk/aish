//! Minimal human handoff domain helpers（0055）。

use crate::domain::shell_single_quote;

pub const HANDOFF_ENV_KEYS: [&str; 4] = [
    "AISH_CONTROL_MODE",
    "AISH_HANDOFF_PARENT_REQUEST",
    "AISH_HANDOFF_SUGGESTED_COMMAND",
    "AISH_HANDOFF_RUNTIME_DIR",
];

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
}
