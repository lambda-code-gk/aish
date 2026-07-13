//! `ai ask` 起動元の分類（TTY 対話入力の入口判定用）。

use std::ffi::OsString;

/// bare `ai` / explicit `ai ask` / implicit message などの起動元。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskInvocationSource {
    /// `argv` が `ai` のみ（サブコマンド・メッセージなし）
    BareRoot,
    /// `ai ask ...`
    ExplicitAsk,
    /// `ai hello` のようにメッセージを伴う implicit ask
    ImplicitMessage,
    /// `ai --preset foo` や `ai ask --dry-run` のように option-only
    OptionOnly,
}

pub fn is_known_cli_head(word: &str) -> bool {
    matches!(
        word,
        "ask"
            | "collab"
            | "chat"
            | "retry"
            | "rerun"
            | "history"
            | "status"
            | "doctor"
            | "ping"
            | "smart"
            | "complete"
            | "recall"
            | "goal"
            | "now"
            | "idea"
            | "mem"
            | "context"
            | "work"
            | "help"
            | "-h"
            | "--help"
            | "-V"
            | "--version"
    )
}

/// 正規化前の raw `argv` から ask 起動元を分類する。
pub fn classify_ask_invocation(args: &[OsString]) -> AskInvocationSource {
    if args.len() <= 1 {
        return AskInvocationSource::BareRoot;
    }

    let first = args[1].to_string_lossy();
    if first == "ask" {
        return AskInvocationSource::ExplicitAsk;
    }
    if is_known_cli_head(&first) {
        // ask 経路では使わないが、呼び出し側でガードする
        return AskInvocationSource::ExplicitAsk;
    }
    if first.starts_with('-') {
        return AskInvocationSource::OptionOnly;
    }
    AskInvocationSource::ImplicitMessage
}

/// 対話的プロンプト入力モードに入ってよいか。
pub fn should_enter_interactive_prompt_mode(
    invocation: AskInvocationSource,
    stdin_is_tty: bool,
) -> bool {
    stdin_is_tty && invocation == AskInvocationSource::BareRoot
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn os_vec(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn classify_bare_root() {
        assert_eq!(
            classify_ask_invocation(&os_vec(&["ai"])),
            AskInvocationSource::BareRoot
        );
    }

    #[test]
    fn classify_explicit_ask() {
        assert_eq!(
            classify_ask_invocation(&os_vec(&["ai", "ask", "hello"])),
            AskInvocationSource::ExplicitAsk
        );
    }

    #[test]
    fn classify_implicit_message() {
        assert_eq!(
            classify_ask_invocation(&os_vec(&["ai", "hello"])),
            AskInvocationSource::ImplicitMessage
        );
    }

    #[test]
    fn classify_option_only() {
        assert_eq!(
            classify_ask_invocation(&os_vec(&["ai", "--preset", "x"])),
            AskInvocationSource::OptionOnly
        );
    }

    #[test]
    fn interactive_prompt_only_for_bare_root_tty() {
        assert!(should_enter_interactive_prompt_mode(
            AskInvocationSource::BareRoot,
            true
        ));
        assert!(!should_enter_interactive_prompt_mode(
            AskInvocationSource::BareRoot,
            false
        ));
        assert!(!should_enter_interactive_prompt_mode(
            AskInvocationSource::ExplicitAsk,
            true
        ));
        assert!(!should_enter_interactive_prompt_mode(
            AskInvocationSource::ImplicitMessage,
            true
        ));
        assert!(!should_enter_interactive_prompt_mode(
            AskInvocationSource::OptionOnly,
            true
        ));
    }
}
