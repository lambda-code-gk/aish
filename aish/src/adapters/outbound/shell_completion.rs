//! `aish shell` 子プロセス向けの一時 rcfile 生成。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BASH_COLLABORATIVE_PROMPT_SNIPPET: &str = r#"
_aish_collaborative_install_prompt() {
  [[ -n "${_AISH_COLLAB_PROMPT_READY:-}" ]] && return 0
  [[ "${AISH_CONTROL_MODE:-}" == human-shell ]] || return 0
  command -v aish >/dev/null 2>&1 || return 0
  _AISH_COLLAB_PROMPT_READY=1
  _AISH_BASE_PS1="${PS1:-\$ }"
  PS1='$(aish collaborative-prompt 2>/dev/null)'"${_AISH_BASE_PS1}"
}
"#;

const BASH_SNIPPET: &str = r#"if command -v aish >/dev/null 2>&1; then eval "$(aish complete bash)"; fi
if command -v ai >/dev/null 2>&1; then eval "$(ai complete bash)"; fi
if command -v aibe >/dev/null 2>&1; then eval "$(aibe complete bash)"; fi
"#;

const BASH_RECALL_ENV_SNIPPET: &str = r#"
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

const BASH_REPLAY_SNIPPET: &str = r#"
_aish_json_escape() {
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\r'/\\r}
  s=${s//$'\t'/\\t}
  printf '"%s"' "$s"
}
_aish_replay_emit() {
  local fifo="${AISH_CONTROL_FIFO:-}"
  [[ -n "$fifo" && -p "$fifo" ]] || return 0
  printf '%s\n' "$1" >"$fifo" 2>/dev/null || true
}
_aish_replay_debug() {
  [[ -n "${AISH_CONTROL_FIFO:-}" ]] || return 0
  [[ -n "${_AISH_LINE_STARTED:-}" ]] && return 0
  case "$BASH_COMMAND" in
    _aish_*|trap*|":" ) return ;;
  esac
  local line="${READLINE_LINE:-$BASH_COMMAND}"
  _aish_replay_emit "{\"event\":\"start\",\"command\":$(_aish_json_escape "$line")}"
  _AISH_LINE_STARTED=1
}
_aish_replay_precmd() {
  local code=$?
  if [[ -n "${AISH_CONTROL_FIFO:-}" && -n "${_AISH_LINE_STARTED:-}" ]]; then
    _aish_replay_emit "{\"event\":\"end\",\"exit_code\":$code}"
    unset _AISH_LINE_STARTED
  fi
}
_aish_replay_install_hooks() {
  [[ -n "${_AISH_REPLAY_READY:-}" ]] && return 0
  _AISH_REPLAY_READY=1
  trap '_aish_replay_debug' DEBUG
}
_aish_handoff_return() {
  local code=$?
  [[ "${AISH_CONTROL_MODE:-}" == human-shell ]] || return 0
  _aish_replay_emit "{\"event\":\"human_return\",\"exit_code\":$code,\"cwd\":$(_aish_json_escape "$PWD")}" || true
}
if [[ -n "${AISH_CONTROL_FIFO:-}" && $- == *i* ]]; then
  # rcfile 評価中に DEBUG が有効だと直後の PROMPT_COMMAND 代入自体が span 化され control pipe が詰まる。
  trap - DEBUG 2>/dev/null || true
  PROMPT_COMMAND="_aish_collaborative_install_prompt;_aish_replay_install_hooks;_aish_replay_precmd${PROMPT_COMMAND:+;}$PROMPT_COMMAND"
  trap '_aish_handoff_return' EXIT
fi
"#;

const ZSH_COLLABORATIVE_PROMPT_SNIPPET: &str = r#"
_aish_collaborative_install_prompt() {
  [[ -n "${_AISH_COLLAB_PROMPT_READY:-}" ]] && return
  [[ "${AISH_CONTROL_MODE:-}" == human-shell ]] || return
  command -v aish >/dev/null 2>&1 || return
  _AISH_COLLAB_PROMPT_READY=1
  _AISH_BASE_PS1="${PS1:-%# }"
  PS1='$(aish collaborative-prompt 2>/dev/null)'"${_AISH_BASE_PS1}"
}
"#;

const ZSH_SNIPPET: &str = r#"if command -v aish >/dev/null 2>&1; then eval "$(aish complete zsh)"; fi
if command -v ai >/dev/null 2>&1; then eval "$(ai complete zsh)"; fi
if command -v aibe >/dev/null 2>&1; then eval "$(aibe complete zsh)"; fi
"#;

const ZSH_RECALL_ENV_SNIPPET: &str = BASH_RECALL_ENV_SNIPPET;

const ZSH_REPLAY_SNIPPET: &str = r#"
_aish_json_escape() {
  local s=$1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\r'/\\r}
  s=${s//$'\t'/\\t}
  printf '"%s"' "$s"
}
_aish_replay_emit() {
  local fifo="${AISH_CONTROL_FIFO:-}"
  [[ -n "$fifo" && -p "$fifo" ]] || return 0
  printf '%s\n' "$1" >"$fifo" 2>/dev/null || true
}
_aish_replay_preexec() {
  emulate -L zsh
  [[ -n "${AISH_CONTROL_FIFO:-}" ]] || return
  _aish_replay_emit "{\"event\":\"start\",\"command\":$(_aish_json_escape "$1")}"
  _AISH_HAVE_START=1
}
_aish_replay_precmd() {
  local code=$?
  [[ -n "${AISH_CONTROL_FIFO:-}" && -n "${_AISH_HAVE_START:-}" ]] || return
  _aish_replay_emit "{\"event\":\"end\",\"exit_code\":$code}"
  unset _AISH_HAVE_START
}
_aish_replay_install_hooks() {
  [[ -n "${_AISH_REPLAY_READY:-}" ]] && return
  _AISH_REPLAY_READY=1
  preexec_functions+=(_aish_replay_preexec)
  precmd_functions+=(_aish_replay_precmd)
}
_aish_handoff_return() {
  local code=$?
  [[ "${AISH_CONTROL_MODE:-}" == human-shell ]] || return
  _aish_replay_emit "{\"event\":\"human_return\",\"exit_code\":$code,\"cwd\":$(_aish_json_escape "$PWD")}" || true
}
if [[ -n "${AISH_CONTROL_FIFO:-}" ]]; then
  precmd_functions+=(_aish_collaborative_install_prompt)
  precmd_functions+=(_aish_replay_install_hooks)
  zshexit_functions+=(_aish_handoff_return)
fi
"#;

/// 子シェル種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildShellKind {
    Bash,
    Zsh,
    Other,
}

pub fn detect_child_shell(shell_path: &str) -> ChildShellKind {
    let base = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    match base {
        "bash" | "bash-bin" => ChildShellKind::Bash,
        "zsh" => ChildShellKind::Zsh,
        _ => ChildShellKind::Other,
    }
}

/// bash / zsh 用の一時 rcfile を生成する。`Other` のときは `None`。
pub fn prepare_interactive_rc(shell_path: &str) -> io::Result<Option<ShellRcLayout>> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    match detect_child_shell(shell_path) {
        ChildShellKind::Bash => {
            let dir = tempfile::tempdir()?;
            let rc = dir.path().join("aish-bashrc");
            let user_rc = PathBuf::from(&home).join(".bashrc");
            write_bash_wrapper(&rc, &user_rc)?;
            Ok(Some(ShellRcLayout {
                _dir: dir,
                bash_rcfile: Some(rc),
                zdotdir: None,
            }))
        }
        ChildShellKind::Zsh => {
            let dir = tempfile::tempdir()?;
            let zdot = dir.path().join("zdotdir");
            fs::create_dir(&zdot)?;
            let zshrc = zdot.join(".zshrc");
            let user_zshrc = PathBuf::from(&home).join(".zshrc");
            write_zsh_wrapper(&zshrc, &user_zshrc)?;
            Ok(Some(ShellRcLayout {
                _dir: dir,
                bash_rcfile: None,
                zdotdir: Some(zdot),
            }))
        }
        ChildShellKind::Other => Ok(None),
    }
}

pub struct ShellRcLayout {
    _dir: tempfile::TempDir,
    pub bash_rcfile: Option<PathBuf>,
    pub zdotdir: Option<PathBuf>,
}

fn write_bash_wrapper(dst: &Path, user_rc: &Path) -> io::Result<()> {
    let mut body = String::new();
    if user_rc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_rc),
            shell_quote(user_rc)
        ));
    }
    body.push_str(BASH_RECALL_ENV_SNIPPET);
    body.push_str(BASH_SNIPPET);
    body.push_str(BASH_COLLABORATIVE_PROMPT_SNIPPET);
    body.push_str(BASH_REPLAY_SNIPPET);
    fs::write(dst, body)
}

fn write_zsh_wrapper(dst: &Path, user_zshrc: &Path) -> io::Result<()> {
    let mut body = String::new();
    if user_zshrc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_zshrc),
            shell_quote(user_zshrc)
        ));
    }
    body.push_str(ZSH_RECALL_ENV_SNIPPET);
    body.push_str(ZSH_SNIPPET);
    body.push_str(ZSH_COLLABORATIVE_PROMPT_SNIPPET);
    body.push_str(ZSH_REPLAY_SNIPPET);
    fs::write(dst, body)
}

fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bash_and_zsh() {
        assert_eq!(detect_child_shell("/bin/bash"), ChildShellKind::Bash);
        assert_eq!(detect_child_shell("/usr/bin/zsh"), ChildShellKind::Zsh);
        assert_eq!(detect_child_shell("/bin/fish"), ChildShellKind::Other);
    }

    #[test]
    fn bash_wrapper_includes_snippet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join("rc");
        write_bash_wrapper(&rc, Path::new("/nonexistent/.bashrc")).expect("write");
        let content = fs::read_to_string(rc).expect("read");
        assert!(content.contains("aish complete bash"));
        assert!(content.contains("AI_SUGGESTION_CACHE"));
    }

    #[test]
    fn bash_replay_hooks_defer_debug_trap_until_first_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join("rc");
        write_bash_wrapper(&rc, Path::new("/nonexistent/.bashrc")).expect("write");
        let content = fs::read_to_string(rc).expect("read");
        assert!(content.contains("_aish_replay_install_hooks"));
        assert!(content.contains("trap - DEBUG"));
        let before_install = content
            .split("_aish_replay_install_hooks()")
            .next()
            .expect("install_hooks");
        assert!(
            !before_install.contains("trap '_aish_replay_debug' DEBUG"),
            "DEBUG trap must not be set before deferred install"
        );
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(
            shell_quote(Path::new("/home/foo'bar/.bashrc")),
            "'/home/foo'\\''bar/.bashrc'"
        );
    }
}
