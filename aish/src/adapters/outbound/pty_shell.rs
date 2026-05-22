//! PTY 対話シェルアダプタ（`libc::openpty`）。

use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};

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
    fn run_shell(&mut self, shell: &str) -> Result<i32, InteractiveShellError> {
        let shell_c =
            CString::new(shell).map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        let arg_i = CString::new("-i").map_err(|e| InteractiveShellError::Failed(e.to_string()))?;

        let (master, slave) = open_pty_pair()?;

        match unsafe { libc::fork() } {
            -1 => Err(InteractiveShellError::Failed("fork failed".into())),
            0 => {
                child_exec_shell(master, slave, &shell_c, &arg_i);
                unreachable!();
            }
            child => {
                unsafe {
                    libc::close(slave);
                }
                let master_fd = master;
                let stdin_thread = std::thread::spawn(move || {
                    copy_stdin_to_fd(master_fd);
                });
                let code = relay_master_fd(master, child, self)?;
                let _ = stdin_thread.join();
                Ok(code)
            }
        }
    }
}

/// fork 子専用。`?` やパニックで親に戻らないこと。
fn child_exec_shell(master: RawFd, slave: RawFd, shell: &CString, arg_i: &CString) {
    unsafe {
        libc::close(master);
    }

    if let Err(e) = setup_controlling_tty(slave) {
        child_die(&e.to_string());
    }

    let argv = [shell.as_ptr(), arg_i.as_ptr(), std::ptr::null()];
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

fn copy_stdin_to_fd(fd: RawFd) {
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 1024];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut written = 0;
                while written < n {
                    let rc =
                        unsafe { libc::write(fd, buf[written..n].as_ptr().cast(), n - written) };
                    if rc <= 0 {
                        return;
                    }
                    written += rc as usize;
                }
            }
            Err(_) => break,
        }
    }
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
            flush_line(pty, &mut line_buf)?;
            return Ok(status);
        }

        match master_file.read(&mut buf) {
            Ok(0) => {
                flush_line(pty, &mut line_buf)?;
                return wait_blocking(child);
            }
            Ok(n) => {
                let chunk = &buf[..n];
                stdout
                    .write_all(chunk)
                    .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
                stdout
                    .flush()
                    .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
                for ch in String::from_utf8_lossy(chunk).chars() {
                    if ch == '\n' || ch == '\r' {
                        flush_line(pty, &mut line_buf)?;
                    } else {
                        line_buf.push(ch);
                    }
                }
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

fn wait_nonblocking(child: libc::pid_t) -> Result<Option<i32>, InteractiveShellError> {
    let mut status: libc::c_int = 0;
    let pid = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
    if pid == 0 {
        return Ok(None);
    }
    if pid < 0 {
        return Err(InteractiveShellError::Failed(format!(
            "waitpid: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(Some(exit_code_from_status(status)))
}

fn wait_blocking(child: libc::pid_t) -> Result<i32, InteractiveShellError> {
    let mut status: libc::c_int = 0;
    let pid = unsafe { libc::waitpid(child, &mut status, 0) };
    if pid < 0 {
        return Err(InteractiveShellError::Failed(format!(
            "waitpid: {}",
            std::io::Error::last_os_error()
        )));
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
