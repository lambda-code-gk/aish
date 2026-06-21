//! ask 起動元の分類（domain への薄い委譲）。

use crate::domain::{classify_ask_invocation, AskInvocationSource};

pub fn classify_from_raw_args(args: &[std::ffi::OsString]) -> AskInvocationSource {
    classify_ask_invocation(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{should_enter_interactive_prompt_mode, AskInvocationSource};
    use std::ffi::OsString;

    fn os_vec(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn unit_explicit_invocations_do_not_enter_prompt_mode() {
        for invocation in [
            AskInvocationSource::ExplicitAsk,
            AskInvocationSource::ImplicitMessage,
            AskInvocationSource::OptionOnly,
        ] {
            assert!(!should_enter_interactive_prompt_mode(invocation, true));
        }
    }

    #[test]
    fn unit_bare_ai_tty_starts_prompt_mode_classification() {
        let invocation = classify_from_raw_args(&os_vec(&["ai"]));
        assert_eq!(invocation, AskInvocationSource::BareRoot);
        assert!(should_enter_interactive_prompt_mode(invocation, true));
    }
}
