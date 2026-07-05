//! `ai complete` と `aish shell` が共有する recall hook 文字列。

/// bash / zsh 向け recall hook trailer。idempotent。
pub const BASH_RECALL_HOOK: &str = r#"
# ai suggested-command recall (0053)
_ai_recall_ready=0
_ai_recall_apply() {
  local cmd=$1
  [[ -n "$cmd" ]] || return 0
  READLINE_LINE="$cmd"
  READLINE_POINT=${#cmd}
}
_ai_recall_next() {
  local cache="${AI_SUGGESTION_CACHE:-}"
  [[ -n "$cache" ]] || return 0
  local cmd
  cmd=$(AI_SUGGESTION_CACHE="$cache" ai recall next 2>/dev/null) || return 0
  _ai_recall_apply "$cmd"
}
_ai_recall_prev() {
  local cache="${AI_SUGGESTION_CACHE:-}"
  [[ -n "$cache" ]] || return 0
  local cmd
  cmd=$(AI_SUGGESTION_CACHE="$cache" ai recall prev 2>/dev/null) || return 0
  _ai_recall_apply "$cmd"
}
_ai_recall_install() {
  [[ "${AI_SUGGESTED_COMMAND_RECALL:-1}" == "0" ]] && return 0
  [[ "$_ai_recall_ready" == "1" ]] && return 0
  _ai_recall_ready=1
  bind -x '"\e.": "_ai_recall_next"' 2>/dev/null || true
  bind -x '"\e,": "_ai_recall_prev"' 2>/dev/null || true
}
if [[ $- == *i* ]]; then
  _ai_recall_install
fi
"#;

pub const ZSH_RECALL_HOOK: &str = r#"
# ai suggested-command recall (0053)
typeset -g _AI_RECALL_READY=0
_ai_recall_apply() {
  local cmd=$1
  [[ -n "$cmd" ]] || return 0
  BUFFER="$cmd"
  CURSOR=${#cmd}
  zle reset-prompt
}
_ai_recall_next() {
  emulate -L zsh
  local cache="${AI_SUGGESTION_CACHE:-}"
  [[ -n "$cache" ]] || return 0
  local cmd
  cmd=$(AI_SUGGESTION_CACHE="$cache" ai recall next 2>/dev/null) || return 0
  _ai_recall_apply "$cmd"
}
_ai_recall_prev() {
  emulate -L zsh
  local cache="${AI_SUGGESTION_CACHE:-}"
  [[ -n "$cache" ]] || return 0
  local cmd
  cmd=$(AI_SUGGESTION_CACHE="$cache" ai recall prev 2>/dev/null) || return 0
  _ai_recall_apply "$cmd"
}
_ai_recall_install() {
  [[ "${AI_SUGGESTED_COMMAND_RECALL:-1}" == "0" ]] && return 0
  [[ "$_AI_RECALL_READY" == "1" ]] && return 0
  _AI_RECALL_READY=1
  zle -N _ai_recall_next
  zle -N _ai_recall_prev
  bindkey '\e.' _ai_recall_next
  bindkey '\e,' _ai_recall_prev
}
if [[ -o interactive ]]; then
  _ai_recall_install
fi
"#;

pub const BASH_RECALL_ENV_SNIPPET: &str = r#"
_ai_recall_export_env() {
  local sid="${AI_SESSION_ID:-}"
  [[ -n "$sid" ]] || return 0
  local home="${HOME:-/tmp}"
  export AI_SUGGESTION_CACHE="${AI_SUGGESTION_CACHE:-${home}/.local/share/ai/suggestions/${sid}.json}"
  export AI_SUGGESTED_COMMAND_RECALL="${AI_SUGGESTED_COMMAND_RECALL:-1}"
  export AI_SUGGESTED_COMMAND_RECALL_HINT="${AI_SUGGESTED_COMMAND_RECALL_HINT:-1}"
}
_ai_recall_export_env
"#;

pub const ZSH_RECALL_ENV_SNIPPET: &str = BASH_RECALL_ENV_SNIPPET;

pub fn recall_hook_for_shell(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" => Some(BASH_RECALL_HOOK),
        "zsh" => Some(ZSH_RECALL_HOOK),
        _ => None,
    }
}

pub fn recall_env_snippet_for_shell(shell: &str) -> Option<&'static str> {
    match shell {
        "bash" | "zsh" => Some(BASH_RECALL_ENV_SNIPPET),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_recall_env_snippet_preserves_existing_cache_path() {
        assert!(BASH_RECALL_ENV_SNIPPET.contains("AI_SUGGESTION_CACHE:-"));
        assert!(!BASH_RECALL_ENV_SNIPPET.contains(
            "export AI_SUGGESTION_CACHE=\"${home}/.local/share/ai/suggestions/${sid}.json\""
        ));
    }

    #[test]
    fn bash_alt_period_inserts_suggested_command_into_readline_line() {
        assert!(BASH_RECALL_HOOK.contains(r#"bind -x '"\e.": "_ai_recall_next"'"#));
        assert!(BASH_RECALL_HOOK.contains(r#"bind -x '"\e,": "_ai_recall_prev"'"#));
        assert!(BASH_RECALL_HOOK.contains("READLINE_LINE"));
        assert!(!BASH_RECALL_HOOK.contains("history -s"));
    }

    #[test]
    fn zsh_alt_period_inserts_suggested_command_into_buffer() {
        assert!(ZSH_RECALL_HOOK.contains(r#"bindkey '\e.' _ai_recall_next"#));
        assert!(ZSH_RECALL_HOOK.contains(r#"bindkey '\e,' _ai_recall_prev"#));
        assert!(ZSH_RECALL_HOOK.contains("BUFFER="));
        assert!(!ZSH_RECALL_HOOK.contains("history -s"));
    }

    #[test]
    fn aish_shell_and_ai_complete_install_the_same_recall_hook() {
        assert_eq!(recall_hook_for_shell("bash"), Some(BASH_RECALL_HOOK));
        assert_eq!(recall_hook_for_shell("zsh"), Some(ZSH_RECALL_HOOK));
    }

    #[test]
    fn unsupported_shells_do_not_install_recall_hook() {
        assert!(recall_hook_for_shell("fish").is_none());
        assert!(recall_env_snippet_for_shell("fish").is_none());
    }
}
