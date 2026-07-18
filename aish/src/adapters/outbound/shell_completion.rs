//! `aish shell` 子プロセス向けの一時 rcfile 生成。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BASH_SNIPPET: &str = r#"if command -v aish >/dev/null 2>&1; then eval "$(aish complete bash)"; fi
if command -v ai >/dev/null 2>&1; then eval "$(ai complete bash)"; fi
if command -v aibe >/dev/null 2>&1; then eval "$(aibe complete bash)"; fi
"#;

const BASH_RECALL_ENV_SNIPPET: &str = r#"
_ai_recall_export_env() {
  local sid="${AI_SESSION_ID:-}"
  [[ -n "$sid" ]] || return 0
  local home="${HOME:-/tmp}"
  export AI_SUGGESTION_CACHE="${home}/.local/share/ai/suggestions/${sid}.json"
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
  ( printf '%s\n' "$1" >"$fifo" ) >/dev/null 2>&1 &
  local wpid=$!
  # 通常は wait。停滞時のみ別 watchdog が writer を打ち切る（busy spin しない）。
  (
    sleep 0.5
    kill "$wpid" 2>/dev/null || true
  ) >/dev/null 2>&1 &
  local wdog=$!
  wait "$wpid" 2>/dev/null
  local emit_status=$?
  kill "$wdog" 2>/dev/null || true
  wait "$wdog" 2>/dev/null || true
  return "$emit_status"
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
  [[ "${_AISH_HUMAN_SHELL:-}" == 1 ]] || return 0
  _aish_replay_emit "{\"event\":\"human_return\",\"exit_code\":$code,\"cwd\":$(_aish_json_escape "$PWD")}" || true
}
human-task() {
  [[ "${_AISH_EXPLICIT_HUMAN_TASK:-}" == 1 && "${1:-}" == suspend && $# -le 2 ]] || { printf '%s\n' 'usage: human-task suspend [reason]' >&2; return 2; }
  local reason="${2:-}"
  local byte_len
  byte_len=$(LC_ALL=C printf '%s' "$reason" | wc -c) || return 1
  [[ "$byte_len" -le 4096 ]] || { printf '%s\n' 'suspend reason exceeds 4096 bytes' >&2; return 2; }
  command aish __human-task-suspend --reason "$reason" --cwd "$PWD" || return 1
  trap - EXIT
  exit 0
}
if [[ -n "${AISH_CONTROL_FIFO:-}" && $- == *i* ]]; then
  # rcfile 評価中に DEBUG が有効だと直後の PROMPT_COMMAND 代入自体が span 化され control pipe が詰まる。
  trap - DEBUG 2>/dev/null || true
  # exit_code を壊さないよう、$? を読む precmd を install_hooks より先に置く。
  PROMPT_COMMAND="_aish_replay_precmd;_aish_replay_install_hooks${PROMPT_COMMAND:+;}$PROMPT_COMMAND"
  trap '_aish_handoff_return' EXIT
fi
"#;

const BASH_HANDOFF_EXIT_SNIPPET: &str = r#"
# 0055 minimal human handoff: 1 回の Ctrl+D / exit で親へ戻る
if [[ $- == *i* && "${_AISH_HUMAN_SHELL:-}" == 1 ]]; then
  set +o ignoreeof 2>/dev/null || true
  _aish_handoff_on_hup() {
    trap - EXIT
    exit 129
  }
  trap _aish_handoff_on_hup HUP
fi
"#;

const BASH_HANDOFF_ENV_STRIP_SNIPPET: &str = r#"
# 0055 minimal human handoff: 対話 shell へ handoff 環境変数を渡さない
if [[ "${AISH_CONTROL_MODE:-}" == human-shell ]]; then
  _AISH_HUMAN_SHELL=1
  [[ -n "${AISH_HANDOFF_TASK_JSON:-}" ]] && _AISH_EXPLICIT_HUMAN_TASK=1
  _AISH_HANDOFF_SUGGESTED_COMMAND="${AISH_HANDOFF_SUGGESTED_COMMAND:-}"
  unset AISH_CONTROL_MODE AISH_HANDOFF_PARENT_REQUEST AISH_HANDOFF_SUGGESTED_COMMAND AISH_HANDOFF_RUNTIME_DIR AISH_HANDOFF_TASK_JSON
fi
"#;

const BASH_HANDOFF_RECALL_SNIPPET: &str = r#"
# 0055 minimal human handoff: suggested command を prompt へ挿入（実行はしない）
_aish_handoff_recall_apply() {
  local cmd="${_AISH_HANDOFF_SUGGESTED_COMMAND:-}"
  [[ -n "$cmd" ]] || return 0
  READLINE_LINE="$cmd"
  READLINE_POINT=${#cmd}
}
_aish_handoff_recall_install() {
  [[ "${_AISH_HUMAN_SHELL:-}" == 1 ]] || return 0
  [[ -n "${_AISH_HANDOFF_SUGGESTED_COMMAND:-}" ]] || return 0
  [[ "${_AISH_HANDOFF_RECALL_READY:-}" == 1 ]] && return 0
  _AISH_HANDOFF_RECALL_READY=1
  bind -x '"\e.": "_aish_handoff_recall_apply"' 2>/dev/null || true
  bind -x '"\e,": "_aish_handoff_recall_apply"' 2>/dev/null || true
}
if [[ $- == *i* ]]; then
  _aish_handoff_recall_install
fi
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
  ( printf '%s\n' "$1" >"$fifo" ) >/dev/null 2>&1 &
  local wpid=$!
  # 通常は wait。停滞時のみ別 watchdog が writer を打ち切る（busy spin しない）。
  (
    sleep 0.5
    kill "$wpid" 2>/dev/null || true
  ) >/dev/null 2>&1 &
  local wdog=$!
  wait "$wpid" 2>/dev/null
  local emit_status=$?
  kill "$wdog" 2>/dev/null || true
  wait "$wdog" 2>/dev/null || true
  return "$emit_status"
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
  # $? を壊さないよう、precmd を先頭へ入れ、install 自身は外す。
  precmd_functions=(_aish_replay_precmd ${precmd_functions:#_aish_replay_install_hooks})
}
_aish_handoff_return() {
  local code=$?
  [[ "${_AISH_HUMAN_SHELL:-}" == 1 ]] || return
  _aish_replay_emit "{\"event\":\"human_return\",\"exit_code\":$code,\"cwd\":$(_aish_json_escape "$PWD")}" || true
}
human-task() {
  emulate -L zsh
  [[ "${_AISH_EXPLICIT_HUMAN_TASK:-}" == 1 && "${1:-}" == suspend && $# -le 2 ]] || { print -u2 'usage: human-task suspend [reason]'; return 2; }
  local reason="${2:-}"
  local byte_len=$(LC_ALL=C printf '%s' "$reason" | wc -c) || return 1
  [[ "$byte_len" -le 4096 ]] || { print -u2 'suspend reason exceeds 4096 bytes'; return 2; }
  command aish __human-task-suspend --reason "$reason" --cwd "$PWD" || return 1
  zshexit_functions=(${zshexit_functions:#_aish_handoff_return})
  exit 0
}
if [[ -n "${AISH_CONTROL_FIFO:-}" ]]; then
  precmd_functions+=(_aish_replay_install_hooks)
  zshexit_functions+=(_aish_handoff_return)
fi
"#;

const ZSH_HANDOFF_EXIT_SNIPPET: &str = r#"
# 0055 minimal human handoff: 1 回の Ctrl+D / exit で親へ戻る
if [[ "${_AISH_HUMAN_SHELL:-}" == 1 ]]; then
  setopt NO_CHECK_JOBS 2>/dev/null || true
fi
if [[ -o interactive && "${_AISH_HUMAN_SHELL:-}" == 1 ]]; then
  unsetopt IGNORE_EOF 2>/dev/null || true
  _aish_handoff_on_hup() {
    zshexit_functions=(${zshexit_functions:#_aish_handoff_return})
    exit 129
  }
  trap _aish_handoff_on_hup HUP
fi
"#;

const ZSH_HANDOFF_ENV_STRIP_SNIPPET: &str = r#"
# 0055 minimal human handoff: 対話 shell へ handoff 環境変数を渡さない
if [[ "${AISH_CONTROL_MODE:-}" == human-shell ]]; then
  _AISH_HUMAN_SHELL=1
  [[ -n "${AISH_HANDOFF_TASK_JSON:-}" ]] && _AISH_EXPLICIT_HUMAN_TASK=1
  _AISH_HANDOFF_SUGGESTED_COMMAND="${AISH_HANDOFF_SUGGESTED_COMMAND:-}"
  unset AISH_CONTROL_MODE AISH_HANDOFF_PARENT_REQUEST AISH_HANDOFF_SUGGESTED_COMMAND AISH_HANDOFF_RUNTIME_DIR AISH_HANDOFF_TASK_JSON
fi
"#;

const ZSH_HANDOFF_RECALL_SNIPPET: &str = r#"
# 0055 minimal human handoff: suggested command を prompt へ挿入（実行はしない）
_aish_handoff_recall_apply() {
  emulate -L zsh
  local cmd="${_AISH_HANDOFF_SUGGESTED_COMMAND:-}"
  [[ -n "$cmd" ]] || return 0
  BUFFER="$cmd"
  CURSOR=${#cmd}
  # reset-prompt は prompt 再展開で line editor を乱し得るため、再描画のみ行う（0067）
  zle -R
}
_aish_handoff_recall_install() {
  [[ "${_AISH_HUMAN_SHELL:-}" == 1 ]] || return 0
  [[ -n "${_AISH_HANDOFF_SUGGESTED_COMMAND:-}" ]] || return 0
  [[ "$_AISH_HANDOFF_RECALL_READY" == 1 ]] && return 0
  _AISH_HANDOFF_RECALL_READY=1
  zle -N _aish_handoff_recall_apply
  bindkey '\e.' _aish_handoff_recall_apply
  bindkey '\e,' _aish_handoff_recall_apply
}
if [[ -o interactive ]]; then
  _aish_handoff_recall_install
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
    prepare_interactive_rc_with_home(shell_path, Path::new(&home))
}

fn prepare_interactive_rc_with_home(
    shell_path: &str,
    home: &Path,
) -> io::Result<Option<ShellRcLayout>> {
    match detect_child_shell(shell_path) {
        ChildShellKind::Bash => {
            let dir = tempfile::tempdir()?;
            let rc = dir.path().join("aish-bashrc");
            let user_rc = home.join(".bashrc");
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
            // Debian/Ubuntu の /etc/zsh/zshrc は compinit を呼び、compaudit の対話確認で
            // non-TTY / CI handoff がハングしうる。ZDOTDIR/.zshenv でだけ無効化する。
            fs::write(
                zdot.join(".zshenv"),
                concat!(
                    "# Generated by aish: avoid global compinit hang during human handoff\n",
                    "if [[ \"${AISH_CONTROL_MODE:-}\" == human-shell ]]; then\n",
                    "  skip_global_compinit=1\n",
                    "fi\n",
                ),
            )?;
            let zshrc = zdot.join(".zshrc");
            let user_zshrc = home.join(".zshrc");
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
    body.push_str(BASH_HANDOFF_ENV_STRIP_SNIPPET);
    if user_rc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_rc),
            shell_quote(user_rc)
        ));
    }
    body.push_str(BASH_HANDOFF_EXIT_SNIPPET);
    body.push_str(BASH_SNIPPET);
    body.push_str(BASH_RECALL_ENV_SNIPPET);
    body.push_str(BASH_REPLAY_SNIPPET);
    body.push_str(BASH_HANDOFF_RECALL_SNIPPET);
    fs::write(dst, body)
}

fn write_zsh_wrapper(dst: &Path, user_zshrc: &Path) -> io::Result<()> {
    let mut body = String::new();
    body.push_str(ZSH_HANDOFF_ENV_STRIP_SNIPPET);
    // /etc/zsh/zshrc 処理後・ユーザー ~/.zshrc の前にクリアし、補完初期化スキップが漏れないようにする。
    body.push_str("unset skip_global_compinit\n");
    if user_zshrc.is_file() {
        body.push_str(&format!(
            "if [ -f {} ]; then . {}; fi\n",
            shell_quote(user_zshrc),
            shell_quote(user_zshrc)
        ));
    }
    body.push_str(ZSH_HANDOFF_EXIT_SNIPPET);
    body.push_str(ZSH_SNIPPET);
    body.push_str(ZSH_RECALL_ENV_SNIPPET);
    body.push_str(ZSH_REPLAY_SNIPPET);
    body.push_str(ZSH_HANDOFF_RECALL_SNIPPET);
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
    fn bash_wrapper_disables_ignoreeof_for_human_handoff() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join("rc");
        write_bash_wrapper(&rc, Path::new("/nonexistent/.bashrc")).expect("write");
        let content = fs::read_to_string(rc).expect("read");
        assert!(content.contains("set +o ignoreeof"));
        assert!(content.contains("_AISH_HUMAN_SHELL"));
    }

    #[test]
    fn zsh_wrapper_disables_ignore_eof_for_human_handoff() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zshrc = dir.path().join(".zshrc");
        write_zsh_wrapper(&zshrc, Path::new("/nonexistent/.zshrc")).expect("write");
        let content = fs::read_to_string(zshrc).expect("read");
        assert!(content.contains("unsetopt IGNORE_EOF"));
        assert!(content.contains("setopt NO_CHECK_JOBS"));
        assert!(content.contains("_AISH_HUMAN_SHELL"));
    }

    #[test]
    fn zsh_zdotdir_skips_global_compinit_for_human_handoff() {
        let layout = prepare_interactive_rc("/usr/bin/zsh")
            .expect("prepare")
            .expect("zsh layout");
        let zdot = layout.zdotdir.as_ref().expect("zdotdir");
        let zshenv = fs::read_to_string(zdot.join(".zshenv")).expect("zshenv");
        assert!(zshenv.contains("skip_global_compinit=1"));
        assert!(zshenv.contains("AISH_CONTROL_MODE"));
        assert!(zdot.join(".zshrc").is_file());
    }

    #[test]
    fn zsh_wrapper_unsets_skip_global_compinit_before_user_zshrc() {
        let dir = tempfile::tempdir().expect("tempdir");
        let user = dir.path().join("user.zshrc");
        fs::write(&user, "# user rc\n").expect("write user");
        let zshrc = dir.path().join(".zshrc");
        write_zsh_wrapper(&zshrc, &user).expect("write wrapper");
        let content = fs::read_to_string(zshrc).expect("read");
        let unset_at = content
            .find("unset skip_global_compinit")
            .expect("must unset skip_global_compinit");
        let user_at = content
            .find(&shell_quote(&user))
            .expect("must source user zshrc");
        assert!(
            unset_at < user_at,
            "unset skip_global_compinit must precede user .zshrc source"
        );
        let strip_at = content
            .find("unset AISH_CONTROL_MODE")
            .expect("handoff env strip");
        assert!(
            strip_at < unset_at,
            "unset skip_global_compinit must follow handoff env strip"
        );
    }

    #[test]
    fn bash_wrapper_includes_snippet() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rc = dir.path().join("rc");
        write_bash_wrapper(&rc, Path::new("/nonexistent/.bashrc")).expect("write");
        let content = fs::read_to_string(rc).expect("read");
        assert!(content.contains("aish complete bash"));
        assert!(content.contains("AI_SUGGESTION_CACHE"));
        assert!(content.contains("AISH_HANDOFF_SUGGESTED_COMMAND"));
        assert!(content.contains(r#"bind -x '"\e.": "_aish_handoff_recall_apply"'"#));
        assert!(!content.contains("stty sane"));
    }

    #[test]
    fn zsh_wrapper_includes_handoff_recall_hook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let zshrc = dir.path().join(".zshrc");
        write_zsh_wrapper(&zshrc, Path::new("/nonexistent/.zshrc")).expect("write");
        let content = fs::read_to_string(zshrc).expect("read");
        assert!(content.contains("AISH_HANDOFF_SUGGESTED_COMMAND"));
        assert!(content.contains(r#"bindkey '\e.' _aish_handoff_recall_apply"#));
        assert!(content.contains("zle -R"));
        assert!(!content.contains("zle reset-prompt"));
        assert!(!content.contains("stty sane"));
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
        assert!(
            content.contains(r#"PROMPT_COMMAND="_aish_replay_precmd;_aish_replay_install_hooks"#),
            "precmd must run before install_hooks so exit_code is preserved"
        );
    }

    #[test]
    fn replay_emit_writes_control_fifo_with_wait_and_watchdog() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bash_rc = dir.path().join("bashrc");
        let zsh_rc = dir.path().join("zshrc");
        write_bash_wrapper(&bash_rc, Path::new("/nonexistent/.bashrc")).expect("bash");
        write_zsh_wrapper(&zsh_rc, Path::new("/nonexistent/.zshrc")).expect("zsh");
        let bash = fs::read_to_string(bash_rc).expect("read bash");
        let zsh = fs::read_to_string(zsh_rc).expect("read zsh");
        for content in [&bash, &zsh] {
            assert!(
                content.contains(r#"( printf '%s\n' "$1" >"$fifo" ) >/dev/null 2>&1 &"#),
                "replay emit must background the fifo write"
            );
            assert!(
                content.contains(r#"wait "$wpid""#),
                "replay emit must wait on the writer (no busy spin)"
            );
            assert!(
                content.contains(r#"sleep 0.5"#),
                "replay emit must use a separate watchdog sleep"
            );
            assert!(
                content.contains(r#"kill "$wpid""#),
                "replay emit watchdog must kill a stalled writer"
            );
            assert!(
                !content.contains(r#"-lt 1000"#),
                "replay emit must not busy-spin on kill -0"
            );
            assert!(
                !content.contains(r#"disown "$!""#),
                "replay emit must not disown without waiting"
            );
        }
    }

    #[test]
    fn replay_emit_completes_quickly_when_reader_is_alive() {
        use std::ffi::CString;
        use std::io::{BufRead, BufReader};
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let fifo = dir.path().join("control.fifo");
        let path_c = CString::new(fifo.as_os_str().as_bytes()).expect("cpath");
        assert_eq!(
            unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) },
            0,
            "mkfifo failed"
        );

        let fifo_reader = fifo.clone();
        let reader = std::thread::spawn(move || {
            let file = std::fs::File::open(&fifo_reader).expect("open reader");
            let mut reader = BufReader::new(file);
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
        });

        // reader が open するまで少し待つ
        std::thread::sleep(std::time::Duration::from_millis(20));

        let bash_rc = dir.path().join("bashrc");
        write_bash_wrapper(&bash_rc, Path::new("/nonexistent/.bashrc")).expect("bash rc");
        // 本番生成 snippet から _aish_replay_emit を抜き出して実行する（コピー禁止）。
        let script = format!(
            r#"
set -euo pipefail
eval "$(sed -n '/^_aish_replay_emit()/,/^}}/p' {rc})"
AISH_CONTROL_FIFO={fifo}
start=$(date +%s%N)
_aish_replay_emit '{{"event":"end","exit_code":0}}'
end=$(date +%s%N)
elapsed_ms=$(( (end - start) / 1000000 ))
echo "$elapsed_ms"
"#,
            rc = shell_quote(&bash_rc),
            fifo = shell_quote(&fifo)
        );
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("run emit timing");
        assert!(
            output.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        reader.join().expect("reader");
        let elapsed_ms: u64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .expect("parse elapsed");
        assert!(
            elapsed_ms < 50,
            "healthy-reader emit must finish quickly, took {elapsed_ms}ms"
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
