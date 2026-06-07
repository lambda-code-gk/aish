//! PTY 対話シェルアダプタ（`libc::openpty`）。

use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

use crate::adapters::outbound::ShellRcLayout;
use crate::domain::sanitize_log_text;
use crate::ports::outbound::{InteractiveShellError, InteractiveShellRunner, SessionLog};

/// マスタ PTY と stdin/stdout を中継し、出力を `SessionLog` に追記する。
pub struct PtyShell<'a, L: SessionLog> {
    log: &'a mut L,
}

impl<'a, L: SessionLog> PtyShell<'a, L> {
    pub fn new(log: &'a mut L) -> Self {
        Self { log }
    }

    fn append_stdout(&mut self, data: &str) -> Result<(), InteractiveShellError> {
        if data.is_empty() {
            return Ok(());
        }
        self.log
            .append(&crate::domain::LogEvent::Stdout {
                data: sanitize_log_text(data),
            })
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))
    }
}

impl<L: SessionLog> InteractiveShellRunner for PtyShell<'_, L> {
    fn run_shell(&mut self, shell: &str, session_dir: &Path) -> Result<i32, InteractiveShellError> {
        let shell_c =
            CString::new(shell).map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        let session_dir = session_dir
            .canonicalize()
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        let session_dir_c = CString::new(session_dir.into_os_string().into_vec())
            .map_err(|_| InteractiveShellError::Failed("AISH_SESSION_DIR contains NUL".into()))?;

        let (master, slave) = open_pty_pair()?;
        let rc_layout = crate::adapters::outbound::prepare_interactive_rc(shell)
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;

        match unsafe { libc::fork() } {
            -1 => Err(InteractiveShellError::Failed("fork failed".into())),
            0 => {
                child_exec_shell(master, slave, &shell_c, &session_dir_c, rc_layout.as_ref());
                unreachable!();
            }
            child => {
                unsafe {
                    libc::close(slave);
                }
                run_shell_parent(master, child, self)
            }
        }
    }
}

fn run_shell_parent<L: SessionLog>(
    master: RawFd,
    child: libc::pid_t,
    pty: &mut PtyShell<'_, L>,
) -> Result<i32, InteractiveShellError> {
    // 親 TTY のローカル echo と PTY 内シェルの echo が重ならないよう raw にする。
    let _stdin_tty =
        StdinTermiosGuard::enter_raw().map_err(|e| fail_after_fork(child, master, [], e))?;

    let stdin_master = dup_pty_master(master).map_err(|e| fail_after_fork(child, master, [], e))?;

    let (shutdown_read, shutdown_write) =
        open_shutdown_pipe().map_err(|e| fail_after_fork(child, master, [stdin_master], e))?;

    let stdin_thread = std::thread::spawn(move || {
        relay_stdin_to_pty(libc::STDIN_FILENO, stdin_master, shutdown_read);
    });

    let relay_result = relay_master_fd(master, child, pty);
    signal_stdin_relay_shutdown(shutdown_write);
    stdin_thread.join().expect("stdin relay thread panicked");
    relay_result
}

/// fork 後のセットアップ失敗時: 追加 fd を閉じ、`master` を閉じ、子を終了して回収する。
fn fail_after_fork(
    child: libc::pid_t,
    master: RawFd,
    extra_fds: impl IntoIterator<Item = RawFd>,
    err: InteractiveShellError,
) -> InteractiveShellError {
    for fd in extra_fds {
        close_raw_fd(fd);
    }
    close_raw_fd(master);
    kill_and_wait(child);
    err
}

/// 子を終了させて reap する（失敗経路・`ESRCH` は無視）。
fn kill_and_wait(child: libc::pid_t) {
    unsafe {
        libc::kill(child, libc::SIGTERM);
    }
    let mut status: libc::c_int = 0;
    loop {
        match waitpid_loop(child, &mut status, 0) {
            Ok(pid) if pid > 0 => break,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

fn os_error_is_eintr() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
}

fn waitpid_loop(
    child: libc::pid_t,
    status: &mut libc::c_int,
    options: i32,
) -> Result<libc::pid_t, InteractiveShellError> {
    loop {
        let pid = unsafe { libc::waitpid(child, status, options) };
        if pid < 0 {
            if os_error_is_eintr() {
                continue;
            }
            return Err(InteractiveShellError::Failed(format!(
                "waitpid: {}",
                std::io::Error::last_os_error()
            )));
        }
        return Ok(pid);
    }
}

/// fork 子専用。`?` やパニックで親に戻らないこと。
fn child_exec_shell(
    master: RawFd,
    slave: RawFd,
    shell: &CString,
    session_dir: &CString,
    rc_layout: Option<&ShellRcLayout>,
) {
    unsafe {
        libc::close(master);
    }

    if let Err(e) = setup_controlling_tty(slave) {
        child_die(&e.to_string());
    }

    let ask_log_key = CString::new("AI_ASK_LOG").expect("static key");
    let ask_log_value = CString::new("session").expect("static value");
    let rc = unsafe { libc::setenv(ask_log_key.as_ptr(), ask_log_value.as_ptr(), 1) };
    if rc != 0 {
        child_die(&format!(
            "setenv(AI_ASK_LOG): {}",
            std::io::Error::last_os_error()
        ));
    }

    let key = CString::new("AISH_SESSION_DIR").expect("static key");
    let rc = unsafe { libc::setenv(key.as_ptr(), session_dir.as_ptr(), 1) };
    if rc != 0 {
        child_die(&format!(
            "setenv(AISH_SESSION_DIR): {}",
            std::io::Error::last_os_error()
        ));
    }

    if let Some(session_id) = session_dir
        .as_c_str()
        .to_str()
        .ok()
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|n| n.to_str())
    {
        let ai_session_key = CString::new("AI_SESSION_ID").expect("static key");
        let ai_session_value = CString::new(session_id).expect("session id");
        let rc = unsafe { libc::setenv(ai_session_key.as_ptr(), ai_session_value.as_ptr(), 1) };
        if rc != 0 {
            child_die(&format!(
                "setenv(AI_SESSION_ID): {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    if let Some(layout) = rc_layout {
        if let Some(zdot) = layout.zdotdir.as_ref() {
            let zdot_c = path_to_cstring(zdot);
            let zdot_key = CString::new("ZDOTDIR").expect("static key");
            let rc = unsafe { libc::setenv(zdot_key.as_ptr(), zdot_c.as_ptr(), 1) };
            if rc != 0 {
                child_die(&format!(
                    "setenv(ZDOTDIR): {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
    }

    let arg_i = CString::new("-i").expect("static -i");
    let mut argv: Vec<*const libc::c_char> = vec![shell.as_ptr()];

    let rcfile_c = rc_layout
        .and_then(|l| l.bash_rcfile.as_ref())
        .map(|p| path_to_cstring(p));
    let dash_rcfile = CString::new("--rcfile").expect("static --rcfile");
    if let Some(ref rcfile) = rcfile_c {
        // `--rcfile` must precede `-i`; `bash -i --rcfile …` is rejected as an invalid `--`.
        argv.push(dash_rcfile.as_ptr());
        argv.push(rcfile.as_ptr());
    }
    argv.push(arg_i.as_ptr());
    argv.push(std::ptr::null());

    unsafe {
        libc::close(slave);
        if libc::execvp(shell.as_ptr(), argv.as_ptr()) == -1 {
            child_die(&format!(
                "execvp({}): {}",
                shell.to_string_lossy(),
                std::io::Error::last_os_error()
            ));
        }
    }
}

fn path_to_cstring(path: &Path) -> CString {
    CString::new(path.as_os_str().as_bytes().to_vec())
        .unwrap_or_else(|_| CString::new("/").expect("fallback path"))
}

fn child_die(msg: &str) -> ! {
    let line = format!("aish: {msg}\n");
    unsafe {
        libc::write(libc::STDERR_FILENO, line.as_ptr().cast(), line.len());
        libc::_exit(1);
    }
}

fn open_pty_pair() -> Result<(RawFd, RawFd), InteractiveShellError> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let rc = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(InteractiveShellError::Failed(format!(
            "openpty failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok((master, slave))
}

fn dup_pty_master(master: RawFd) -> Result<RawFd, InteractiveShellError> {
    let duped = unsafe { libc::dup(master) };
    if duped < 0 {
        return Err(InteractiveShellError::Failed(format!(
            "dup(master): {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(duped)
}

/// 対話 TTY 上の stdin を raw にし、PTY 経由の echo のみ表示する。非 TTY では何もしない。
struct StdinTermiosGuard {
    active: bool,
    original: libc::termios,
}

impl StdinTermiosGuard {
    fn enter_raw() -> Result<Self, InteractiveShellError> {
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
            return Ok(Self {
                active: false,
                original: unsafe { std::mem::zeroed() },
            });
        }

        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return Err(InteractiveShellError::Failed(format!(
                "tcgetattr(stdin): {}",
                std::io::Error::last_os_error()
            )));
        }

        let mut raw = original;
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) } != 0 {
            return Err(InteractiveShellError::Failed(format!(
                "tcsetattr(stdin, raw): {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(Self {
            active: true,
            original,
        })
    }
}

impl Drop for StdinTermiosGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.original) };
        }
    }
}

fn open_shutdown_pipe() -> Result<(RawFd, RawFd), InteractiveShellError> {
    let mut fds = [-1i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(InteractiveShellError::Failed(format!(
            "pipe: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok((fds[0], fds[1]))
}

/// 親が shell 終了後に shutdown pipe 書き端を close し、stdin 中継スレッドの `poll` を unblock する。
fn signal_stdin_relay_shutdown(shutdown_write_fd: RawFd) {
    close_raw_fd(shutdown_write_fd);
}

/// stdin（またはテスト用 fd）を PTY master（dup 済み）へ中継する。`shutdown_read_fd` の書き端 close で終了する。
fn relay_stdin_to_pty(stdin_fd: RawFd, stdin_master: RawFd, shutdown_read_fd: RawFd) {
    let mut buf = [0u8; 1024];
    loop {
        let mut fds = [
            libc::pollfd {
                fd: stdin_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: shutdown_read_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let rc = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if rc < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        if rc == 0 {
            continue;
        }

        if fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            break;
        }

        if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            let n = match read_fd(stdin_fd, &mut buf) {
                Ok(n) => n,
                Err(_) => break,
            };
            if n <= 0 {
                break;
            }
            if !write_all_fd(stdin_master, &buf[..n as usize]) {
                break;
            }
        }
    }

    close_raw_fd(stdin_master);
    close_raw_fd(shutdown_read_fd);
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> Result<isize, InteractiveShellError> {
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            if os_error_is_eintr() {
                continue;
            }
            return Err(InteractiveShellError::Failed(format!(
                "read: {}",
                std::io::Error::last_os_error()
            )));
        }
        return Ok(n);
    }
}

fn write_all_fd(fd: RawFd, buf: &[u8]) -> bool {
    let mut written = 0;
    while written < buf.len() {
        let rc = loop {
            let rc =
                unsafe { libc::write(fd, buf[written..].as_ptr().cast(), buf.len() - written) };
            if rc < 0 {
                if os_error_is_eintr() {
                    continue;
                }
                return false;
            }
            break rc;
        };
        if rc == 0 {
            return false;
        }
        written += rc as usize;
    }
    true
}

fn close_raw_fd(fd: RawFd) {
    if fd >= 0 {
        let _ = unsafe { libc::close(fd) };
    }
}

/// 子プロセスを新しいセッションのリーダーにし、スレーブ PTY を制御端末にする。
fn setup_controlling_tty(slave: RawFd) -> Result<(), InteractiveShellError> {
    if unsafe { libc::setsid() } == -1 {
        return Err(InteractiveShellError::Failed(format!(
            "setsid: {}",
            std::io::Error::last_os_error()
        )));
    }

    let rc = unsafe { libc::ioctl(slave, libc::TIOCSCTTY, 0) };
    if rc != 0 {
        return Err(InteractiveShellError::Failed(format!(
            "TIOCSCTTY: {}",
            std::io::Error::last_os_error()
        )));
    }

    for stdfd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(slave, stdfd) } < 0 {
            return Err(InteractiveShellError::Failed(format!(
                "dup2: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    Ok(())
}

fn relay_master_fd<L: SessionLog>(
    master: RawFd,
    child: libc::pid_t,
    pty: &mut PtyShell<'_, L>,
) -> Result<i32, InteractiveShellError> {
    let mut master_file = unsafe { std::fs::File::from_raw_fd(master) };
    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();

    loop {
        if let Some(status) = wait_nonblocking(child)? {
            drain_master_output(&mut master_file, &mut stdout, pty, &mut line_buf, &mut buf)?;
            flush_line(pty, &mut line_buf)?;
            return Ok(status);
        }

        match master_file.read(&mut buf) {
            Ok(0) => {
                flush_line(pty, &mut line_buf)?;
                return wait_blocking(child);
            }
            Ok(n) => {
                relay_master_chunk(&buf[..n], &mut stdout, pty, &mut line_buf)?;
            }
            Err(e) => {
                flush_line(pty, &mut line_buf)?;
                if e.raw_os_error() == Some(libc::EIO) {
                    return wait_blocking(child);
                }
                return Err(InteractiveShellError::Failed(e.to_string()));
            }
        }
    }
}

fn relay_master_chunk<L: SessionLog>(
    chunk: &[u8],
    stdout: &mut std::io::StdoutLock<'_>,
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
) -> Result<(), InteractiveShellError> {
    stdout
        .write_all(chunk)
        .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
    stdout
        .flush()
        .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
    for ch in String::from_utf8_lossy(chunk).chars() {
        if ch == '\n' || ch == '\r' {
            flush_line(pty, line_buf)?;
        } else {
            line_buf.push(ch);
        }
    }
    Ok(())
}

fn drain_master_output<L: SessionLog>(
    master_file: &mut std::fs::File,
    stdout: &mut std::io::StdoutLock<'_>,
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
    buf: &mut [u8],
) -> Result<(), InteractiveShellError> {
    loop {
        match master_file.read(buf) {
            Ok(0) => break,
            Ok(n) => relay_master_chunk(&buf[..n], stdout, pty, line_buf)?,
            Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
            Err(e) => return Err(InteractiveShellError::Failed(e.to_string())),
        }
    }
    Ok(())
}

fn wait_nonblocking(child: libc::pid_t) -> Result<Option<i32>, InteractiveShellError> {
    let mut status: libc::c_int = 0;
    let pid = waitpid_loop(child, &mut status, libc::WNOHANG)?;
    if pid == 0 {
        return Ok(None);
    }
    Ok(Some(exit_code_from_status(status)))
}

fn wait_blocking(child: libc::pid_t) -> Result<i32, InteractiveShellError> {
    let mut status: libc::c_int = 0;
    let pid = waitpid_loop(child, &mut status, 0)?;
    if pid <= 0 {
        return Err(InteractiveShellError::Failed(
            "waitpid returned no child".into(),
        ));
    }
    Ok(exit_code_from_status(status))
}

fn exit_code_from_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status) as i32
    } else {
        1
    }
}

fn flush_line<L: SessionLog>(
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
) -> Result<(), InteractiveShellError> {
    if !line_buf.is_empty() {
        pty.append_stdout(line_buf)?;
        line_buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::FromRawFd;
    use std::thread;
    use std::time::{Duration, Instant};

    fn pipe_pair() -> (RawFd, RawFd) {
        let mut fds = [-1i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    #[test]
    fn fail_after_fork_kills_and_reaps_child() {
        let child = match unsafe { libc::fork() } {
            -1 => panic!("fork failed"),
            0 => {
                unsafe {
                    libc::sleep(3600);
                }
                std::process::exit(0);
            }
            pid => pid,
        };

        let (master, slave) = open_pty_pair().expect("openpty");
        unsafe {
            libc::close(slave);
        }

        let _ = fail_after_fork(
            child,
            master,
            [],
            InteractiveShellError::Failed("test abort".into()),
        );

        let mut status: libc::c_int = 0;
        let pid = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
        assert!(pid <= 0, "child should be reaped (waitpid returned {pid})");
    }

    #[test]
    fn stdin_relay_exits_after_shutdown_pipe_closed() {
        let (input_read, _input_write) = pipe_pair();
        let (shutdown_read, shutdown_write) = open_shutdown_pipe().expect("shutdown pipe");
        let (_pty_read, pty_write) = pipe_pair();

        let relay_handle = thread::spawn(move || {
            relay_stdin_to_pty(input_read, pty_write, shutdown_read);
        });

        signal_stdin_relay_shutdown(shutdown_write);

        let started = Instant::now();
        relay_handle.join().expect("relay thread panicked");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "join blocked too long after shutdown"
        );
    }

    #[test]
    fn stdin_relay_forwards_input_to_pty() {
        let (input_read, input_write) = pipe_pair();
        let (shutdown_read, shutdown_write) = open_shutdown_pipe().expect("shutdown pipe");
        let (pty_read, pty_write) = pipe_pair();

        let relay_handle = thread::spawn(move || {
            relay_stdin_to_pty(input_read, pty_write, shutdown_read);
        });

        {
            let mut input_file = unsafe { std::fs::File::from_raw_fd(input_write) };
            input_file.write_all(b"hello").expect("write input");
        }

        relay_handle.join().expect("relay thread panicked");
        close_raw_fd(shutdown_write);

        let mut output = String::new();
        unsafe { std::fs::File::from_raw_fd(pty_read) }
            .read_to_string(&mut output)
            .expect("read pty output");
        assert_eq!(output, "hello");
    }

    #[test]
    fn stdin_relay_exits_on_stdin_eof_without_shutdown() {
        let (input_read, input_write) = pipe_pair();
        let (shutdown_read, shutdown_write) = open_shutdown_pipe().expect("shutdown pipe");
        let (_pty_read, pty_write) = pipe_pair();

        let relay_handle = thread::spawn(move || {
            relay_stdin_to_pty(input_read, pty_write, shutdown_read);
        });

        close_raw_fd(input_write);

        let started = Instant::now();
        relay_handle.join().expect("relay thread panicked");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "join blocked too long after stdin EOF"
        );

        close_raw_fd(shutdown_write);
    }
}
