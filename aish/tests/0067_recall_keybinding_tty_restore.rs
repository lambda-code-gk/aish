#![cfg(unix)]
//! 0067 Human Shell handoff recall acceptance test.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use aish::adapters::outbound::{prepare_interactive_rc, ShellRcLayout};

const PROMPT: &str = "__AISH_0067_HANDOFF__ ";
const TIMEOUT: Duration = Duration::from_secs(8);

/// `prepare_interactive_rc` は process の HOME を読む。テスト binary 内で直列化する。
static PREPARE_RC_HOME_LOCK: Mutex<()> = Mutex::new(());

fn prepare_interactive_rc_under_home(shell_name: &str, home: &Path) -> ShellRcLayout {
    let _guard = PREPARE_RC_HOME_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = std::env::var_os("HOME");
    // SAFETY: 同一 process 内の HOME 変更は上記 Mutex で直列化する。
    unsafe { std::env::set_var("HOME", home) };
    let result = prepare_interactive_rc(shell_name);
    match previous {
        Some(value) => unsafe { std::env::set_var("HOME", value) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    result
        .unwrap_or_else(|error| panic!("prepare {shell_name} rc: {error}"))
        .expect("bash/zsh layout")
}

#[derive(Clone, Copy)]
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

struct PtyShell {
    child: Child,
    master: fs::File,
    slave_fd: i32,
    transcript: Vec<u8>,
    _layout: ShellRcLayout,
}

impl PtyShell {
    fn spawn(shell: Shell, home: &Path, suggestion: &str) -> Self {
        let layout = prepare_interactive_rc_under_home(shell.name(), home);

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
        let mut command = Command::new(shell.name());
        match shell {
            Shell::Bash => {
                let rc = layout.bash_rcfile.as_ref().expect("bash rcfile");
                command.args(["--noprofile", "--rcfile", rc.to_str().unwrap(), "-i"]);
                command.env("PS1", PROMPT);
            }
            Shell::Zsh => {
                command.args(["-d", "-i"]);
                command.env("ZDOTDIR", layout.zdotdir.as_ref().expect("zdotdir"));
                command.env("PROMPT", PROMPT);
            }
        }
        command
            .env("HOME", home)
            .env("PATH", "/usr/bin:/bin")
            .env("AISH_CONTROL_MODE", "human-shell")
            .env("AISH_HANDOFF_SUGGESTED_COMMAND", suggestion)
            .stdin(Stdio::from(slave_file.try_clone().expect("clone stdin")))
            .stdout(Stdio::from(slave_file.try_clone().expect("clone stdout")))
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
            _layout: layout,
        };
        session.expect(PROMPT, "initial handoff prompt");
        session
    }

    fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("write PTY");
        self.master.flush().expect("flush PTY");
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
                Ok(0) => panic!("{label}: PTY closed"),
                Ok(n) => {
                    self.transcript.extend_from_slice(&buffer[..n]);
                    if self.transcript.len() > 32 * 1024 {
                        self.transcript.drain(..16 * 1024);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        panic!(
                            "{label}: timeout for {needle:?}; transcript={:?}",
                            String::from_utf8_lossy(&self.transcript)
                        );
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{label}: PTY read: {error}"),
            }
        }
    }

    fn command(&mut self, command: &str, expected: &str, label: &str) {
        self.send(format!("{command}\n").as_bytes());
        self.expect(&format!("\r\n{expected}\r\n"), label);
        self.expect(PROMPT, label);
    }

    fn action(&mut self, input: &[u8], expected: &str, label: &str) {
        let before = termios(self.slave_fd);
        self.send(input);
        self.expect(&format!("\r\n{expected}\r\n"), label);
        self.expect(PROMPT, label);
        assert_eq!(before, termios(self.slave_fd), "{label}: termios changed");
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

#[test]
fn handoff_recall_preserves_line_editor_navigation() {
    for shell in [Shell::Bash, Shell::Zsh] {
        for (shortcut_name, shortcut) in
            [("next", b"\x1b.".as_slice()), ("prev", b"\x1b,".as_slice())]
        {
            let home = tempfile::tempdir().expect("temp HOME");
            fs::write(
                home.path().join(".bashrc"),
                format!(
                    "PS1='{}'\n_normal_recall() {{ READLINE_LINE='echo NORMAL_BINDING'; READLINE_POINT=${{#READLINE_LINE}}; }}\nbind -x '\"\\e.\": \"_normal_recall\"'\nbind -x '\"\\e,\": \"_normal_recall\"'\n",
                    PROMPT
                ),
            )
            .expect("bashrc");
            fs::write(
                home.path().join(".zshrc"),
                format!(
                    "PROMPT='{}'\n_normal_recall() {{ BUFFER='echo NORMAL_BINDING'; CURSOR=${{#BUFFER}}; zle -R; }}\nzle -N _normal_recall\nbindkey '\\e.' _normal_recall\nbindkey '\\e,' _normal_recall\n",
                    PROMPT
                ),
            )
            .expect("zshrc");
            let mut pty = PtyShell::spawn(shell, home.path(), "echo HANDOFF_CAND");
            let label = format!("{} / {shortcut_name}", shell.name());

            let mut cursor = shortcut.to_vec();
            cursor.extend_from_slice(b"\x1b[D\x1b[D\x1b[CX\n");
            pty.action(
                &cursor,
                "HANDOFF_CANXD",
                &format!("{label} / cursor left+right"),
            );

            pty.command(
                "echo HANDOFF_HIST",
                "HANDOFF_HIST",
                &format!("{label} / seed history"),
            );
            let mut up = shortcut.to_vec();
            up.extend_from_slice(b"\x1b[A\n");
            pty.action(&up, "HANDOFF_HIST", &format!("{label} / history up"));

            pty.command(
                "echo HANDOFF_DOWN",
                "HANDOFF_DOWN",
                &format!("{label} / seed history down"),
            );
            let mut down = shortcut.to_vec();
            down.extend_from_slice(b"\x1b[A\x1b[B\n");
            pty.action(&down, "HANDOFF_CAND", &format!("{label} / history down"));
        }

        let home = tempfile::tempdir().expect("empty-candidate HOME");
        match shell {
            Shell::Bash => fs::write(
                home.path().join(".bashrc"),
                format!(
                    "PS1='{}'\n_normal_recall() {{ READLINE_LINE='echo NORMAL_BINDING'; READLINE_POINT=${{#READLINE_LINE}}; }}\nbind -x '\"\\e.\": \"_normal_recall\"'\n",
                    PROMPT
                ),
            )
            .expect("bash normal binding rc"),
            Shell::Zsh => fs::write(
                home.path().join(".zshrc"),
                format!(
                    "PROMPT='{}'\n_normal_recall() {{ BUFFER='echo NORMAL_BINDING'; CURSOR=${{#BUFFER}}; zle -R; }}\nzle -N _normal_recall\nbindkey '\\e.' _normal_recall\n",
                    PROMPT
                ),
            )
            .expect("zsh normal binding rc"),
        }
        let mut pty = PtyShell::spawn(shell, home.path(), "");
        pty.action(
            b"\x1b.\n",
            "NORMAL_BINDING",
            &format!(
                "{} / no handoff candidate preserves existing binding",
                shell.name()
            ),
        );
    }
}
