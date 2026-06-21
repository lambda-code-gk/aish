//! ask 起動元の分類と対話的プロンプト入力の orchestration。

use crate::domain::{
    classify_ask_invocation, should_enter_interactive_prompt_mode, AskInvocationSource,
};

pub fn classify_from_raw_args(args: &[std::ffi::OsString]) -> AskInvocationSource {
    classify_ask_invocation(args)
}

/// bare `ai` TTY 時にどの prompt 取得経路を使うか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractivePromptRoute {
    ExternalEditor(Vec<String>),
    BuiltinEditor,
}

/// 対話的プロンプト入力が必要なときだけ経路を返す。
/// `editor_command` は composition root（`main`）が env から解決して渡す。
pub fn plan_interactive_prompt_route(
    invocation: AskInvocationSource,
    stdin_is_tty: bool,
    editor_command: Option<Vec<String>>,
) -> Option<InteractivePromptRoute> {
    if !should_enter_interactive_prompt_mode(invocation, stdin_is_tty) {
        return None;
    }

    Some(if let Some(command) = editor_command {
        InteractivePromptRoute::ExternalEditor(command)
    } else {
        InteractivePromptRoute::BuiltinEditor
    })
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

    #[test]
    fn unit_plan_prefers_external_editor_when_configured() {
        let route = plan_interactive_prompt_route(
            AskInvocationSource::BareRoot,
            true,
            Some(vec!["nvim".into()]),
        )
        .expect("route");
        assert_eq!(
            route,
            InteractivePromptRoute::ExternalEditor(vec!["nvim".into()])
        );
    }

    #[test]
    fn unit_plan_falls_back_to_builtin_editor() {
        let route = plan_interactive_prompt_route(AskInvocationSource::BareRoot, true, None)
            .expect("route");
        assert_eq!(route, InteractivePromptRoute::BuiltinEditor);
    }

    #[test]
    fn unit_plan_skips_non_tty_and_non_bare_root() {
        assert!(
            plan_interactive_prompt_route(AskInvocationSource::BareRoot, false, None).is_none()
        );
        assert!(
            plan_interactive_prompt_route(AskInvocationSource::ExplicitAsk, true, None).is_none()
        );
    }
}
