#![cfg(unix)]
//! 0067 Recall Keybinding TTY Restore acceptance tests.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use ai::adapters::outbound::{BASH_RECALL_HOOK, ZSH_RECALL_HOOK};

const PROMPT: &str = "__AISH_0067_PROMPT__ ";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug)]
enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
        }
    }

    fn hook(self) -> &'static str {
        match self {
            Self::Bash => BASH_RECALL_HOOK,
            Self::Zsh => ZSH_RECALL_HOOK,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TermiosSnapshot {
    iflag: libc::tcflag_t,
    oflag: libc::tcflag_t,
    cflag: libc::tcflag_t,
    lflag: libc::tcflag_t,
    cc: Vec<libc::cc_t>,
}

fn termios(fd: i32) -> TermiosSnapshot {
    let mut value: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::tcgetattr(fd, &mut value) }, 0, "tcgetattr");
    TermiosSnapshot {
        iflag: value.c_iflag,
        oflag: value.c_oflag,
        cflag: value.c_cflag,
        lflag: value.c_lflag,
        cc: value.c_cc.to_vec(),
    }
}

fn make_executable(path: &Path) {
    let mut permissions = fs::metadata(path).expect("stub metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(path, permissions).expect("chmod stub");
}

fn write_stub(dir: &Path, mode_file: &Path, log_file: &Path) -> PathBuf {
    let stub = dir.join("ai");
    fs::write(
        &stub,
        format!(
            r#"#!/bin/sh
set -eu
[ "${{1:-}}" = recall ] || exit 64
if IFS= read -r _byte; then stdin_state=data; else stdin_state=eof; fi
cache_path="${{AI_SUGGESTION_CACHE:-}}"
if [ -n "$cache_path" ] && [ ! -f "$cache_path" ]; then
  printf '%s:%s:missing_cache\n' "${{2:-missing}}" "$stdin_state" >> {}
  exit 0
fi
printf '%s:%s:%s\n' "${{2:-missing}}" "$stdin_state" "$(cat {})" >> {}
mode=$(cat {})
case "$mode" in
  success) printf 'echo %s\n' "${{2}}_CAND" ;;
  empty) exit 0 ;;
  failure) printf 'echo SHOULD_NOT_APPLY\n'; exit 23 ;;
  *) exit 64 ;;
esac
"#,
            shell_quote(&log_file.to_string_lossy()),
            shell_quote(&mode_file.to_string_lossy()),
            shell_quote(&log_file.to_string_lossy()),
            shell_quote(&mode_file.to_string_lossy())
        ),
    )
    .expect("write stub ai");
    make_executable(&stub);
    stub
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

struct PtyShell {
    child: Child,
    master: fs::File,
    slave_fd: i32,
    transcript: Vec<u8>,
}

impl PtyShell {
    fn spawn(shell: Shell, home: &Path, stub_dir: &Path, cache: &Path) -> Self {
        let rc = match shell {
            Shell::Bash => {
                let rc = home.join("bashrc");
                fs::write(&rc, format!("PS1='{}'\n{}\n", PROMPT, shell.hook()))
                    .expect("write bash rc");
                rc
            }
            Shell::Zsh => {
                let rc = home.join(".zshrc");
                fs::write(&rc, format!("PROMPT='{}'\n{}\n", PROMPT, shell.hook()))
                    .expect("write zsh rc");
                rc
            }
        };
        let mut master = -1;
        let mut slave = -1;
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0,
            "openpty"
        );
        let retained_slave = unsafe { libc::dup(slave) };
        assert!(retained_slave >= 0, "dup slave");
        let slave_file = unsafe { fs::File::from_raw_fd(slave) };
        let current_path = std::env::var_os("PATH").unwrap_or_default();
        let path = format!("{}:{}", stub_dir.display(), current_path.to_string_lossy());
        let mut command = Command::new(shell.name());
        match shell {
            Shell::Bash => {
                command.args(["--noprofile", "--rcfile", rc.to_str().unwrap(), "-i"]);
            }
            Shell::Zsh => {
                command.args(["-d", "-i"]);
                command.env("ZDOTDIR", home);
            }
        }
        command
            .env("HOME", home)
            .env("PATH", path)
            .env("AI_SUGGESTION_CACHE", cache)
            .stdin(Stdio::from(
                slave_file.try_clone().expect("clone slave stdin"),
            ))
            .stdout(Stdio::from(
                slave_file.try_clone().expect("clone slave stdout"),
            ))
            .stderr(Stdio::from(slave_file));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{} is required for 0067: {error}", shell.name()));
        let master = unsafe { fs::File::from_raw_fd(master) };
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert!(flags >= 0, "fcntl F_GETFL");
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            0,
            "fcntl O_NONBLOCK"
        );
        let mut session = Self {
            child,
            master,
            slave_fd: retained_slave,
            transcript: Vec::new(),
        };
        session.expect(PROMPT, "initial prompt");
        session
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("write PTY input");
        self.master.flush().expect("flush PTY input");
    }

    fn expect(&mut self, needle: &str, label: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(position) = self
                .transcript
                .windows(needle.len())
                .position(|window| window == needle.as_bytes())
            {
                self.transcript.drain(..position + needle.len());
                return;
            }
            let mut buffer = [0_u8; 1024];
            match self.master.read(&mut buffer) {
                Ok(0) => panic!("{label}: PTY closed before {needle:?}"),
                Ok(n) => {
                    self.transcript.extend_from_slice(&buffer[..n]);
                    if self.transcript.len() > 32 * 1024 {
                        self.transcript.drain(..16 * 1024);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        panic!(
                            "{label}: timeout waiting for {needle:?}; transcript={:?}",
                            String::from_utf8_lossy(&self.transcript)
                        );
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{label}: PTY read failed: {error}"),
            }
        }
    }

    fn command(&mut self, command: &str, expected: &str, label: &str) {
        self.send(format!("{command}\n").as_bytes());
        if expected == PROMPT {
            self.expect(PROMPT, label);
        } else {
            self.expect(&format!("\r\n{expected}\r\n"), label);
            self.expect(PROMPT, label);
        }
    }

    fn stable_action(&mut self, input: &[u8], expected: &str, label: &str) {
        let before = termios(self.slave_fd);
        self.send(input);
        self.expect(&format!("\r\n{expected}\r\n"), label);
        self.expect(PROMPT, label);
        let after = termios(self.slave_fd);
        assert_eq!(before, after, "{label}: stable-prompt termios changed");
    }
}

impl Drop for PtyShell {
    fn drop(&mut self) {
        let _ = self.master.write_all(b"exit\n");
        let _ = self.master.flush();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                unsafe { libc::close(self.slave_fd) };
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        unsafe { libc::close(self.slave_fd) };
    }
}

fn set_mode(mode_file: &Path, mode: &str) {
    fs::write(mode_file, mode).expect("write stub mode");
}

#[test]
fn recall_keybinding_pty_vertical_e2e() {
    for shell in [Shell::Bash, Shell::Zsh] {
        for (shortcut_name, shortcut) in
            [("next", b"\x1b.".as_slice()), ("prev", b"\x1b,".as_slice())]
        {
            let temp = tempfile::tempdir().expect("tempdir");
            let mode = temp.path().join("mode");
            let log = temp.path().join("stub.log");
            let cache = temp.path().join("cache.json");
            fs::write(&cache, "{}").expect("cache fixture");
            set_mode(&mode, "success");
            write_stub(temp.path(), &mode, &log);
            let mut pty = PtyShell::spawn(shell, temp.path(), temp.path(), &cache);
            let prefix = format!("{} / {shortcut_name}", shell.name());

            let mut cursor = shortcut.to_vec();
            cursor.extend_from_slice(b"\x1b[D\x1b[D\x1b[CX\n");
            pty.stable_action(
                &cursor,
                &format!("{shortcut_name}_CANXD"),
                &format!("{prefix} / success cursor left+right"),
            );

            pty.command(
                "echo HIST0067",
                "HIST0067",
                &format!("{prefix} / seed history"),
            );
            let mut history_up = shortcut.to_vec();
            history_up.extend_from_slice(b"\x1b[A\n");
            pty.stable_action(
                &history_up,
                "HIST0067",
                &format!("{prefix} / success history up"),
            );

            pty.command(
                "echo HISTDOWN0067",
                "HISTDOWN0067",
                &format!("{prefix} / seed history down"),
            );
            let mut history_down = shortcut.to_vec();
            history_down.extend_from_slice(b"\x1b[A\x1b[B\n");
            pty.stable_action(
                &history_down,
                &format!("{shortcut_name}_CAND"),
                &format!("{prefix} / success history down"),
            );

            set_mode(&mode, "empty");
            let mut empty = b"echo EMPTY_SAFE".to_vec();
            empty.extend_from_slice(shortcut);
            empty.push(b'\n');
            pty.stable_action(&empty, "EMPTY_SAFE", &format!("{prefix} / empty"));

            set_mode(&mode, "failure");
            let mut failure = b"echo FAILURE_SAFE".to_vec();
            failure.extend_from_slice(shortcut);
            failure.push(b'\n');
            pty.stable_action(&failure, "FAILURE_SAFE", &format!("{prefix} / nonzero"));

            // cache パスは維持したままファイルだけ消す（正本の「cache 不在」）。
            // mode=success のまま残し、stub がファイル不在を無視すると候補挿入で失敗する。
            set_mode(&mode, "success");
            let log_before = fs::read_to_string(&log).unwrap_or_default();
            pty.command(
                &format!("rm -f {}", shell_quote(&cache.to_string_lossy())),
                PROMPT,
                &format!("{prefix} / remove cache file"),
            );
            let mut missing = b"echo MISSING_SAFE".to_vec();
            missing.extend_from_slice(shortcut);
            missing.push(b'\n');
            pty.stable_action(
                &missing,
                "MISSING_SAFE",
                &format!("{prefix} / missing cache file"),
            );
            let log_after = fs::read_to_string(&log).expect("stub log after missing cache");
            let new_log = log_after.strip_prefix(&log_before).expect("stub log grew");
            assert!(
                new_log.contains("missing_cache"),
                "{prefix} / missing cache must hit stub missing_cache path; new_log={new_log:?}"
            );
            fs::write(&cache, "{}").expect("restore cache fixture");

            set_mode(&mode, "success");
            pty.stable_action(
                b"\x1b.\x1b,\x1b[DZ\n",
                "prev_CANZD",
                &format!("{prefix} / mixed consecutive input"),
            );
        }
    }
}

#[test]
fn recall_subprocess_cannot_read_widget_input() {
    for shell in [Shell::Bash, Shell::Zsh] {
        for mode in ["success", "empty", "failure"] {
            let temp = tempfile::tempdir().expect("tempdir");
            let mode_file = temp.path().join("mode");
            let log = temp.path().join("stub.log");
            let cache = temp.path().join("cache.json");
            fs::write(&cache, "{}").expect("cache fixture");
            set_mode(&mode_file, mode);
            write_stub(temp.path(), &mode_file, &log);
            let hook = temp.path().join("hook");
            fs::write(&hook, shell.hook()).expect("write hook");
            let current_path = std::env::var_os("PATH").unwrap_or_default();
            let path = format!(
                "{}:{}",
                temp.path().display(),
                current_path.to_string_lossy()
            );
            let script = match shell {
                Shell::Bash => format!(
                    "source {}; READLINE_LINE=original; READLINE_POINT=8; _ai_recall_next; printf '%s|%s\\n' \"$READLINE_LINE\" \"$READLINE_POINT\"",
                    shell_quote(&hook.to_string_lossy())
                ),
                Shell::Zsh => format!(
                    "source {}; BUFFER=original; CURSOR=8; zle() {{ :; }}; _ai_recall_next; printf '%s|%s\\n' \"$BUFFER\" \"$CURSOR\"",
                    shell_quote(&hook.to_string_lossy())
                ),
            };
            let output = Command::new(shell.name())
                .args(["-c", &script])
                .env("PATH", path)
                .env("AI_SUGGESTION_CACHE", &cache)
                .output()
                .unwrap_or_else(|error| panic!("{} is required for 0067: {error}", shell.name()));
            assert!(
                output.status.success(),
                "{} / {mode}: stderr={}",
                shell.name(),
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            if mode == "success" {
                assert!(
                    stdout.contains("echo next_CAND|14"),
                    "{} / {mode}: {stdout}",
                    shell.name()
                );
            } else {
                assert!(
                    stdout.contains("original|8"),
                    "{} / {mode}: {stdout}",
                    shell.name()
                );
                if mode == "failure" {
                    assert!(
                        !stdout.contains("SHOULD_NOT_APPLY"),
                        "{} / failure with stdout must keep buffer: {stdout}",
                        shell.name()
                    );
                }
            }
            assert_eq!(
                fs::read_to_string(&log).expect("stub log").trim(),
                format!("next:eof:{mode}"),
                "{} / {mode}",
                shell.name()
            );
        }
    }
}

#[test]
fn recall_hooks_avoid_unobserved_terminal_reset() {
    for (label, hook) in [
        ("0053 bash", BASH_RECALL_HOOK),
        ("0053 zsh", ZSH_RECALL_HOOK),
    ] {
        assert!(!hook.contains("stty sane"), "{label}");
        assert!(!hook.contains("tcsetattr"), "{label}");
        assert!(!hook.contains("zle reset-prompt"), "{label}");
    }
    let aish_source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../aish/src/adapters/outbound/shell_completion.rs"),
    )
    .expect("read 0055 handoff hook source");
    let aish_product = aish_source
        .split("#[cfg(test)]")
        .next()
        .expect("aish product source");
    assert!(!aish_product.contains("stty sane"));
    assert!(!aish_product.contains("tcsetattr"));
    assert!(!aish_product.contains("zle reset-prompt"));
}
