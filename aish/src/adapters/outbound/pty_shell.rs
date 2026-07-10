//! PTY 対話シェルアダプタ（`libc::openpty`）。

use std::ffi::CString;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::io::{AsRawFd, IntoRawFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::adapters::outbound::ShellRcLayout;
use crate::domain::{rfc3339_now, LogEvent};
use crate::ports::outbound::{InteractiveShellError, InteractiveShellRunner, SessionLog};

/// PTY session の外部 cancel（SIGTERM / SIGINT）要求。
static PTY_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
/// signal handler から poll を起こす self-pipe の書き込み端。
static PTY_CANCEL_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

const PTY_TERM_GRACE: Duration = Duration::from_secs(1);
const PTY_KILL_WAIT: Duration = Duration::from_secs(1);

/// マスタ PTY と stdin/stdout を中継し、出力を `SessionLog` に追記する。
pub struct PtyShell<'a, L: SessionLog> {
    log: &'a mut L,
    human_return: Option<HumanReturnMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HumanReturnMarker {
    pub exit_code: Option<i32>,
    pub final_cwd: String,
}

#[derive(Debug, Default)]
struct CommandSpanState {
    next_index: u32,
    active_index: Option<u32>,
    replay_enabled: bool,
    /// control `start` より先に届いた PTY 出力。入力行 echo が見つかったときだけ、その後ろを span へ移す。
    pending_stdout: String,
    /// span 先頭の「プロンプト + 入力行」echo を replay 記録から除く（hook の command 文字列）
    strip_echo_line: Option<String>,
    pending_end: Option<PendingCommandEnd>,
    queued_start: Option<QueuedCommandStart>,
}

#[derive(Debug)]
struct PendingCommandEnd {
    exit_code: Option<i32>,
    finished_at: String,
}

#[derive(Debug)]
struct QueuedCommandStart {
    index: u32,
    started_at: String,
    command: String,
}

#[derive(Debug, serde::Deserialize)]
struct ControlMessage {
    event: String,
    command: Option<String>,
    exit_code: Option<i32>,
    cwd: Option<String>,
}

impl<'a, L: SessionLog> PtyShell<'a, L> {
    pub fn new(log: &'a mut L) -> Self {
        Self {
            log,
            human_return: None,
        }
    }

    pub fn take_human_return_marker(&mut self) -> Option<HumanReturnMarker> {
        self.human_return.take()
    }

    fn append_stdout(
        &mut self,
        data: &str,
        span: &mut CommandSpanState,
    ) -> Result<(), InteractiveShellError> {
        if data.is_empty() {
            return Ok(());
        }
        if span.active_index.is_some() {
            let (data, index) = self.advance_queued_start_on_echo(data, span)?;
            if data.is_empty() {
                return Ok(());
            }
            let event = LogEvent::stdout_indexed(&data, index);
            self.log
                .append(&event)
                .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        } else if span.replay_enabled {
            span.pending_stdout.push_str(data);
        } else {
            let event = LogEvent::Stdout {
                data: crate::domain::sanitize_log_text(data),
                command_index: None,
            };
            self.log
                .append(&event)
                .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        }
        Ok(())
    }

    fn append_stdout_to_active(
        &mut self,
        data: &str,
        span: &mut CommandSpanState,
    ) -> Result<(), InteractiveShellError> {
        let Some(index) = span.active_index else {
            return Ok(());
        };
        let data = strip_shell_echo_from_span_output(span, data);
        if data.is_empty() {
            return Ok(());
        }
        self.log
            .append(&LogEvent::stdout_indexed(&data, index))
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))
    }

    fn advance_queued_start_on_echo(
        &mut self,
        data: &str,
        span: &mut CommandSpanState,
    ) -> Result<(String, u32), InteractiveShellError> {
        let Some(index) = span.active_index else {
            return Ok((data.to_string(), 0));
        };
        let Some(queued) = span.queued_start.as_ref() else {
            if span.pending_end.is_some() {
                if let Some((before, after)) = split_before_any_prompt_echo(data) {
                    self.append_stdout_to_active(&before, span)?;
                    self.finish_active_span(span)?;
                    span.pending_stdout.push_str(&after);
                    return Ok((String::new(), 0));
                }
            }
            let data = strip_shell_echo_from_span_output(span, data);
            return Ok((data, index));
        };
        let Some((before, after)) = split_before_shell_echo(data, &queued.command) else {
            let data = strip_shell_echo_from_span_output(span, data);
            return Ok((data, index));
        };

        self.append_stdout_to_active(&before, span)?;
        self.finish_active_span(span)?;
        let queued = span.queued_start.take().expect("queued start");
        self.start_span(span, queued.index, queued.started_at, queued.command)?;
        span.strip_echo_line = None;
        let Some(index) = span.active_index else {
            return Ok((String::new(), 0));
        };
        Ok((after, index))
    }

    fn flush_pending_stdout_after_echo(
        &mut self,
        span: &mut CommandSpanState,
        command: &str,
    ) -> Result<(), InteractiveShellError> {
        let Some(index) = span.active_index else {
            return Ok(());
        };
        if span.pending_stdout.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut span.pending_stdout);
        let Some(data) = extract_after_shell_echo(&pending, command) else {
            return Ok(());
        };
        span.strip_echo_line = None;
        if data.is_empty() {
            return Ok(());
        }
        self.log
            .append(&LogEvent::stdout_indexed(&data, index))
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))
    }

    fn start_span(
        &mut self,
        span: &mut CommandSpanState,
        index: u32,
        started_at: String,
        command: String,
    ) -> Result<(), InteractiveShellError> {
        span.active_index = Some(index);
        span.strip_echo_line = Some(command.clone());
        self.log
            .append(&LogEvent::shell_command_start(index, &started_at, &command))
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))
    }

    fn finish_active_span(
        &mut self,
        span: &mut CommandSpanState,
    ) -> Result<(), InteractiveShellError> {
        let Some(index) = span.active_index.take() else {
            return Ok(());
        };
        let Some(end) = span.pending_end.take() else {
            return Ok(());
        };
        span.strip_echo_line = None;
        self.log
            .append(&LogEvent::command_end(
                index,
                end.exit_code,
                &end.finished_at,
            ))
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))
    }

    fn handle_control_line(
        &mut self,
        line: &str,
        span: &mut CommandSpanState,
    ) -> Result<(), InteractiveShellError> {
        if !span.replay_enabled {
            return Ok(());
        }
        let msg: ControlMessage = match serde_json::from_str(line.trim()) {
            Ok(msg) => msg,
            Err(_) => return Ok(()),
        };
        match msg.event.as_str() {
            "start" => {
                if span.active_index.is_some() {
                    return Ok(());
                }
                let Some(command) = msg.command.filter(|c| !c.is_empty()) else {
                    return Ok(());
                };
                span.next_index = span.next_index.saturating_add(1);
                let index = span.next_index;
                let started_at = rfc3339_now();
                if span.active_index.is_some() && span.pending_end.is_some() {
                    span.queued_start = Some(QueuedCommandStart {
                        index,
                        started_at,
                        command,
                    });
                } else {
                    self.start_span(span, index, started_at, command.clone())?;
                    self.flush_pending_stdout_after_echo(span, &command)?;
                }
            }
            "end" => {
                if span.active_index.is_none() {
                    return Ok(());
                }
                span.pending_end = Some(PendingCommandEnd {
                    exit_code: msg.exit_code,
                    finished_at: rfc3339_now(),
                });
            }
            "human_return" => {
                if let Some(cwd) = msg.cwd.filter(|cwd| !cwd.is_empty()) {
                    self.human_return = Some(HumanReturnMarker {
                        exit_code: msg.exit_code,
                        final_cwd: cwd,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl<L: SessionLog> InteractiveShellRunner for PtyShell<'_, L> {
    fn run_shell(&mut self, shell: &str, session_dir: &Path) -> Result<i32, InteractiveShellError> {
        let shell_c =
            CString::new(shell).map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
        let session_dir = session_dir
            .canonicalize()
            .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;

        let (master, slave) = open_pty_pair()?;
        sync_pty_winsize_from_stdin(master)?;
        let rc_layout = match crate::adapters::outbound::prepare_interactive_rc(shell) {
            Ok(layout) => layout,
            Err(err) => {
                eprintln!("aish: replay hooks unavailable: {err}");
                None
            }
        };
        let replay_enabled = rc_layout.is_some();
        let control_channel = if replay_enabled {
            Some(open_control_fifo(&session_dir)?)
        } else {
            None
        };
        let session_dir_c = CString::new(session_dir.into_os_string().into_vec())
            .map_err(|_| InteractiveShellError::Failed("AISH_SESSION_DIR contains NUL".into()))?;

        match unsafe { libc::fork() } {
            -1 => Err(InteractiveShellError::Failed("fork failed".into())),
            0 => {
                child_exec_shell(
                    master,
                    slave,
                    &shell_c,
                    &session_dir_c,
                    rc_layout.as_ref(),
                    control_channel.as_ref().map(|(_, path)| path.as_path()),
                );
                unreachable!();
            }
            child => {
                unsafe {
                    libc::close(slave);
                }
                let control_read = control_channel.map(|(read_fd, fifo_path)| {
                    set_fd_nonblocking(read_fd);
                    (read_fd, fifo_path)
                });
                run_shell_parent(master, child, self, control_read, replay_enabled)
            }
        }
    }
}

fn run_shell_parent<L: SessionLog>(
    master: RawFd,
    child: libc::pid_t,
    pty: &mut PtyShell<'_, L>,
    control_channel: Option<(RawFd, std::path::PathBuf)>,
    replay_enabled: bool,
) -> Result<i32, InteractiveShellError> {
    install_pty_cancel_handlers();
    PTY_CANCEL_REQUESTED.store(false, Ordering::SeqCst);

    // 親 TTY のローカル echo と PTY 内シェルの echo が重ならないよう raw にする。
    let _stdin_tty =
        StdinTermiosGuard::enter_raw().map_err(|e| fail_after_fork(child, master, [], e))?;

    let stdin_master = dup_pty_master(master).map_err(|e| fail_after_fork(child, master, [], e))?;

    let (shutdown_read, shutdown_write) =
        open_shutdown_pipe().map_err(|e| fail_after_fork(child, master, [stdin_master], e))?;

    let (cancel_read, cancel_write) = open_shutdown_pipe().map_err(|e| {
        fail_after_fork(
            child,
            master,
            [stdin_master, shutdown_read, shutdown_write],
            e,
        )
    })?;
    set_fd_nonblocking(cancel_read);
    set_fd_nonblocking(cancel_write);
    PTY_CANCEL_WRITE_FD.store(cancel_write, Ordering::SeqCst);

    let stdin_thread = std::thread::spawn(move || {
        relay_stdin_to_pty(libc::STDIN_FILENO, stdin_master, shutdown_read);
    });

    let control_read = control_channel.as_ref().map(|(fd, _)| *fd);
    let relay_result = relay_master_fd(
        master,
        child,
        pty,
        control_read,
        cancel_read,
        replay_enabled,
    );
    PTY_CANCEL_WRITE_FD.store(-1, Ordering::SeqCst);
    close_raw_fd(cancel_read);
    close_raw_fd(cancel_write);
    if let Some((fd, fifo_path)) = control_channel {
        close_raw_fd(fd);
        let _ = std::fs::remove_file(fifo_path);
    }
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
    kill_and_wait(
        child,
        PTY_TERM_GRACE,
        Instant::now() + PTY_TERM_GRACE + PTY_KILL_WAIT,
    );
    err
}

fn install_pty_cancel_handlers() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = pty_cancel_signal_handler as usize;
        action.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut action.sa_mask);
        let _ = libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
        let _ = libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    });
}

extern "C" fn pty_cancel_signal_handler(_sig: libc::c_int) {
    PTY_CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    let fd = PTY_CANCEL_WRITE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = 1u8;
        unsafe {
            let _ = libc::write(fd, (&raw const byte).cast(), 1);
        }
    }
}

/// PTY shell と同一 session の通常ジョブを有限時間で終了・回収する。
fn terminate_pty_session(
    shell_pid: libc::pid_t,
    master: RawFd,
) -> Result<i32, InteractiveShellError> {
    let deadline = Instant::now() + PTY_TERM_GRACE + PTY_KILL_WAIT;
    let session = unsafe { libc::getsid(shell_pid) };

    // foreground process group へ先に届ける。
    let fg = unsafe { libc::tcgetpgrp(master) };
    if fg > 0 {
        unsafe {
            let _ = libc::kill(-fg, libc::SIGHUP);
            let _ = libc::kill(-fg, libc::SIGTERM);
        }
    }

    // shell の process group。
    let shell_pg = unsafe { libc::getpgid(shell_pid) };
    if shell_pg > 0 {
        unsafe {
            let _ = libc::kill(-shell_pg, libc::SIGHUP);
            let _ = libc::kill(-shell_pg, libc::SIGTERM);
        }
    }
    unsafe {
        let _ = libc::kill(shell_pid, libc::SIGHUP);
        let _ = libc::kill(shell_pid, libc::SIGTERM);
    }

    // master close で HUP/EOF を促す。
    close_raw_fd(master);

    if session > 0 {
        signal_session_pids(session, libc::SIGHUP);
        signal_session_pids(session, libc::SIGTERM);
    }

    let grace_deadline = Instant::now() + PTY_TERM_GRACE;
    while Instant::now() < grace_deadline {
        if let Some(status) = wait_nonblocking(shell_pid)? {
            if session > 0 {
                reap_session_zombies(session);
            }
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if session > 0 {
        signal_session_pids(session, libc::SIGKILL);
    }
    unsafe {
        let _ = libc::kill(shell_pid, libc::SIGKILL);
        if shell_pg > 0 {
            let _ = libc::kill(-shell_pg, libc::SIGKILL);
        }
    }

    let status = wait_with_deadline(shell_pid, deadline)?;
    if session > 0 {
        reap_session_zombies(session);
    }
    Ok(status.unwrap_or(-1))
}

/// PTY master が EOF/EIO になったあとの回収。
/// 子が既に終了していればその status を返し、残存していれば bounded terminate する。
fn reap_after_master_eof(
    child: libc::pid_t,
    master_file: std::fs::File,
) -> Result<i32, InteractiveShellError> {
    let deadline = Instant::now() + PTY_TERM_GRACE;
    if let Some(status) = wait_with_deadline(child, deadline)? {
        return Ok(status);
    }
    let master_raw = master_file.into_raw_fd();
    terminate_pty_session(child, master_raw)
}

/// `relay_master_fd` の一般エラー / cancel / panic で session cleanup を漏らさないための RAII。
///
/// - `needs_terminate == true` のまま Drop → `terminate_pty_session`
/// - 子を既に wait 済みなら `mark_child_reaped`（PID 再利用への誤 signal を避ける）
/// - EOF 経路は `into_master` で所有権を渡し、明示 reap に任せる
struct PtyRelayCleanup {
    child: libc::pid_t,
    master: Option<std::fs::File>,
    needs_terminate: bool,
}

impl PtyRelayCleanup {
    fn new(child: libc::pid_t, master: std::fs::File) -> Self {
        Self {
            child,
            master: Some(master),
            needs_terminate: true,
        }
    }

    fn master_mut(&mut self) -> &mut std::fs::File {
        self.master
            .as_mut()
            .expect("PtyRelayCleanup master still owned")
    }

    fn as_raw_fd(&self) -> RawFd {
        self.master
            .as_ref()
            .expect("PtyRelayCleanup master still owned")
            .as_raw_fd()
    }

    fn mark_child_reaped(&mut self) {
        self.needs_terminate = false;
    }

    fn into_master(mut self) -> (libc::pid_t, std::fs::File) {
        self.needs_terminate = false;
        let child = self.child;
        let master = self
            .master
            .take()
            .expect("PtyRelayCleanup master still owned");
        (child, master)
    }
}

impl Drop for PtyRelayCleanup {
    fn drop(&mut self) {
        let Some(master) = self.master.take() else {
            return;
        };
        if self.needs_terminate {
            let master_raw = master.into_raw_fd();
            let _ = terminate_pty_session(self.child, master_raw);
        }
        // needs_terminate == false のときは File Drop で master を close するだけ。
    }
}

fn signal_session_pids(session: libc::pid_t, signal: i32) {
    for pid in pids_in_session(session) {
        unsafe {
            let _ = libc::kill(pid, signal);
            let pgid = libc::getpgid(pid);
            if pgid > 0 {
                let _ = libc::kill(-pgid, signal);
            }
        }
    }
}

fn pids_in_session(session: libc::pid_t) -> Vec<libc::pid_t> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<libc::pid_t>() else {
            continue;
        };
        let stat_path = entry.path().join("stat");
        let Ok(stat) = std::fs::read_to_string(stat_path) else {
            continue;
        };
        if parse_session_from_stat(&stat) == Some(session) {
            pids.push(pid);
        }
    }
    pids
}

fn parse_session_from_stat(stat: &str) -> Option<libc::pid_t> {
    let rp = stat.rfind(')')?;
    let fields: Vec<&str> = stat[rp + 2..].split_whitespace().collect();
    // After ")": state ppid pgrp session ...
    fields.get(3)?.parse().ok()
}

fn reap_session_zombies(session: libc::pid_t) {
    for pid in pids_in_session(session) {
        let mut status = 0;
        let _ = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    }
}

/// 子を終了させて reap する（失敗経路・`ESRCH` は無視）。有限時間で SIGKILL へ escalate する。
fn kill_and_wait(child: libc::pid_t, grace: Duration, deadline: Instant) {
    unsafe {
        libc::kill(child, libc::SIGTERM);
    }
    let grace_deadline = Instant::now() + grace;
    while Instant::now() < grace_deadline && Instant::now() < deadline {
        let mut status: libc::c_int = 0;
        match waitpid_loop(child, &mut status, libc::WNOHANG) {
            Ok(pid) if pid > 0 => return,
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
    unsafe {
        libc::kill(child, libc::SIGKILL);
    }
    while Instant::now() < deadline {
        let mut status: libc::c_int = 0;
        match waitpid_loop(child, &mut status, libc::WNOHANG) {
            Ok(pid) if pid > 0 => return,
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
}

fn wait_with_deadline(
    child: libc::pid_t,
    deadline: Instant,
) -> Result<Option<i32>, InteractiveShellError> {
    while Instant::now() < deadline {
        if let Some(status) = wait_nonblocking(child)? {
            return Ok(Some(status));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // 最後に一度だけ blocking に近い回収を試みる（短時間）。
    let mut status: libc::c_int = 0;
    match waitpid_loop(child, &mut status, libc::WNOHANG) {
        Ok(pid) if pid > 0 => Ok(Some(exit_code_from_status(status))),
        Ok(_) => Ok(None),
        Err(e) => Err(e),
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
    control_fifo: Option<&Path>,
) {
    unsafe {
        libc::close(master);
    }

    if let Err(e) = setup_controlling_tty(slave) {
        child_die(&e.to_string());
    }

    if let Some(fifo_path) = control_fifo {
        let fifo_key = CString::new("AISH_CONTROL_FIFO").expect("static key");
        let fifo_value = path_to_cstring(fifo_path);
        let rc = unsafe { libc::setenv(fifo_key.as_ptr(), fifo_value.as_ptr(), 1) };
        if rc != 0 {
            child_die(&format!(
                "setenv(AISH_CONTROL_FIFO): {}",
                std::io::Error::last_os_error()
            ));
        }
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

/// 親の実端末（stdin）の winsize を PTY master へコピーする。
///
/// `openpty` 直後は子シェル側の `stty size` が `0 0` になりうるため、fork 前に同期する。
fn sync_pty_winsize_from_stdin(master: RawFd) -> Result<(), InteractiveShellError> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut ws) };
    if rc != 0 || ws.ws_col == 0 || ws.ws_row == 0 {
        return Ok(());
    }
    let rc = unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &ws) };
    if rc != 0 {
        return Err(InteractiveShellError::Failed(format!(
            "TIOCSWINSZ on pty master: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

/// `SIGWINCH` を `signalfd` で受け、親 TTY の winsize を PTY へ伝播する。
struct WinchMonitor {
    fd: RawFd,
    previous_mask: libc::sigset_t,
}

impl WinchMonitor {
    fn install() -> Result<Self, InteractiveShellError> {
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 0 {
            return Err(InteractiveShellError::Failed("stdin is not a tty".into()));
        }

        let mut mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous_mask: libc::sigset_t = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigemptyset(&mut mask) } != 0
            || unsafe { libc::sigaddset(&mut mask, libc::SIGWINCH) } != 0
        {
            return Err(InteractiveShellError::Failed(format!(
                "sigset for SIGWINCH: {}",
                std::io::Error::last_os_error()
            )));
        }
        if unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut previous_mask) } != 0 {
            return Err(InteractiveShellError::Failed(format!(
                "pthread_sigmask(SIG_BLOCK, SIGWINCH): {}",
                std::io::Error::last_os_error()
            )));
        }

        let fd = unsafe { libc::signalfd(-1, &mask, libc::SFD_CLOEXEC) };
        if fd < 0 {
            let err = std::io::Error::last_os_error();
            let _ = unsafe {
                libc::pthread_sigmask(libc::SIG_SETMASK, &previous_mask, std::ptr::null_mut())
            };
            return Err(InteractiveShellError::Failed(format!(
                "signalfd(SIGWINCH): {err}"
            )));
        }

        Ok(Self { fd, previous_mask })
    }

    fn fd(&self) -> RawFd {
        self.fd
    }

    fn drain_and_sync(&self, master: RawFd) -> Result<(), InteractiveShellError> {
        let mut info: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::signalfd_siginfo>();
        loop {
            let n = unsafe {
                libc::read(
                    self.fd,
                    (&mut info as *mut libc::signalfd_siginfo).cast(),
                    size,
                )
            };
            if n < 0 {
                if os_error_is_eintr() {
                    continue;
                }
                return Err(InteractiveShellError::Failed(format!(
                    "read(signalfd): {}",
                    std::io::Error::last_os_error()
                )));
            }
            if n == 0 {
                break;
            }
        }
        sync_pty_winsize_from_stdin(master)
    }
}

impl Drop for WinchMonitor {
    fn drop(&mut self) {
        close_raw_fd(self.fd);
        let _ = unsafe {
            libc::pthread_sigmask(libc::SIG_SETMASK, &self.previous_mask, std::ptr::null_mut())
        };
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
    // 親 aish が SIGKILL 等で突然終了したとき、setsid 済み shell を孤児にしない。
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGHUP) } == -1 {
        return Err(InteractiveShellError::Failed(format!(
            "PR_SET_PDEATHSIG: {}",
            std::io::Error::last_os_error()
        )));
    }

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

fn open_control_fifo(
    session_dir: &Path,
) -> Result<(RawFd, std::path::PathBuf), InteractiveShellError> {
    let fifo_path = session_dir.join("control.fifo");
    if fifo_path.exists() {
        std::fs::remove_file(&fifo_path).map_err(|e| {
            InteractiveShellError::Failed(format!("remove stale control fifo: {e}"))
        })?;
    }
    let path_c = path_to_cstring(&fifo_path);
    if unsafe { libc::mkfifo(path_c.as_ptr(), 0o600) } != 0 {
        return Err(InteractiveShellError::Failed(format!(
            "mkfifo: {}",
            std::io::Error::last_os_error()
        )));
    }
    // O_RDWR: 読み取り専用だと書き込み側がいない間 POLLHUP が常時立ち poll が忙しいループになる。
    // 親は読むだけだが、自プロセスで書き込み端も開いておく（Linux FIFO の定番パターン）。
    let read_fd = unsafe {
        libc::open(
            path_c.as_ptr(),
            libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if read_fd < 0 {
        let _ = std::fs::remove_file(&fifo_path);
        return Err(InteractiveShellError::Failed(format!(
            "open(control fifo): {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok((read_fd, fifo_path))
}

fn set_fd_nonblocking(fd: RawFd) {
    if fd < 0 {
        return;
    }
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return;
    }
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
}

fn relay_master_fd<L: SessionLog>(
    master: RawFd,
    child: libc::pid_t,
    pty: &mut PtyShell<'_, L>,
    control_read: Option<RawFd>,
    cancel_read: RawFd,
    replay_enabled: bool,
) -> Result<i32, InteractiveShellError> {
    let winch = WinchMonitor::install().ok();
    let mut cleanup = PtyRelayCleanup::new(child, unsafe { std::fs::File::from_raw_fd(master) });
    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 4096];
    let mut line_buf = String::new();
    let mut control_buf = String::new();
    let mut span = CommandSpanState {
        next_index: 0,
        active_index: None,
        replay_enabled,
        ..CommandSpanState::default()
    };
    let control_fd = control_read.unwrap_or(-1);

    loop {
        if PTY_CANCEL_REQUESTED.swap(false, Ordering::SeqCst) {
            // Drop(cleanup) が terminate_pty_session を実行する。
            return Err(InteractiveShellError::Failed(
                "pty session cancelled".into(),
            ));
        }

        if let Some(status) = wait_nonblocking(child)? {
            // 子は既に reap 済み。以降の ? 失敗でも PID へ再 signal しない。
            cleanup.mark_child_reaped();
            drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
            drain_master_output(
                cleanup.master_mut(),
                &mut stdout,
                pty,
                &mut line_buf,
                &mut buf,
                &mut span,
            )?;
            flush_line(pty, &mut line_buf, &mut span)?;
            drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
            pty.finish_active_span(&mut span)?;
            return Ok(status);
        }

        let master_raw = cleanup.as_raw_fd();
        let mut poll_fds = vec![libc::pollfd {
            fd: master_raw,
            events: libc::POLLIN,
            revents: 0,
        }];
        if control_fd >= 0 {
            poll_fds.push(libc::pollfd {
                fd: control_fd,
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let winch_index = if let Some(ref monitor) = winch {
            poll_fds.push(libc::pollfd {
                fd: monitor.fd(),
                events: libc::POLLIN,
                revents: 0,
            });
            Some(poll_fds.len() - 1)
        } else {
            None
        };
        let cancel_index = {
            poll_fds.push(libc::pollfd {
                fd: cancel_read,
                events: libc::POLLIN,
                revents: 0,
            });
            poll_fds.len() - 1
        };

        let rc = unsafe { libc::poll(poll_fds.as_mut_ptr(), poll_fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            if os_error_is_eintr() {
                continue;
            }
            // Drop(cleanup) が terminate_pty_session を実行する。
            return Err(InteractiveShellError::Failed(format!(
                "poll(pty master): {}",
                std::io::Error::last_os_error()
            )));
        }

        if poll_fds[cancel_index].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            let mut discard = [0u8; 64];
            loop {
                let n =
                    unsafe { libc::read(cancel_read, discard.as_mut_ptr().cast(), discard.len()) };
                if n <= 0 {
                    break;
                }
            }
            // 次ループ先頭で PTY_CANCEL_REQUESTED を処理する。
            continue;
        }

        if let Some(idx) = winch_index {
            if poll_fds[idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                if let Some(ref monitor) = winch {
                    monitor.drain_and_sync(master_raw)?;
                }
            }
        }

        if control_fd >= 0 {
            let control_idx = 1;
            if poll_fds[control_idx].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
            }
        }

        if poll_fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) == 0 {
            continue;
        }

        if control_fd >= 0 {
            drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
        }

        match cleanup.master_mut().read(&mut buf) {
            Ok(0) => {
                drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
                flush_line(pty, &mut line_buf, &mut span)?;
                drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
                pty.finish_active_span(&mut span)?;
                let (child, master_file) = cleanup.into_master();
                return reap_after_master_eof(child, master_file);
            }
            Ok(n) => {
                relay_master_chunk(&buf[..n], &mut stdout, pty, &mut line_buf, &mut span)?;
            }
            Err(e) => {
                drain_control_input(pty, control_fd, &mut control_buf, &mut span)?;
                flush_line(pty, &mut line_buf, &mut span)?;
                if e.raw_os_error() == Some(libc::EIO) {
                    pty.finish_active_span(&mut span)?;
                    let (child, master_file) = cleanup.into_master();
                    return reap_after_master_eof(child, master_file);
                }
                // Drop(cleanup) が terminate_pty_session を実行する。
                return Err(InteractiveShellError::Failed(e.to_string()));
            }
        }
    }
}

fn drain_control_input<L: SessionLog>(
    pty: &mut PtyShell<'_, L>,
    control_fd: RawFd,
    control_buf: &mut String,
    span: &mut CommandSpanState,
) -> Result<(), InteractiveShellError> {
    if control_fd < 0 {
        return Ok(());
    }
    let mut buf = [0u8; 1024];
    loop {
        let n = unsafe { libc::read(control_fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            if os_error_is_eintr() {
                continue;
            }
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN) {
                break;
            }
            break;
        }
        if n == 0 {
            break;
        }
        control_buf.push_str(&String::from_utf8_lossy(&buf[..n as usize]));
        while let Some(pos) = control_buf.find('\n') {
            let line: String = control_buf.drain(..pos).collect();
            if !control_buf.is_empty() {
                control_buf.remove(0);
            }
            let line = line.trim_end_matches('\r');
            if !line.is_empty() {
                pty.handle_control_line(line, span)?;
            }
        }
    }
    Ok(())
}

fn relay_master_chunk<L: SessionLog>(
    chunk: &[u8],
    stdout: &mut std::io::StdoutLock<'_>,
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
    span: &mut CommandSpanState,
) -> Result<(), InteractiveShellError> {
    stdout
        .write_all(chunk)
        .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
    stdout
        .flush()
        .map_err(|e| InteractiveShellError::Failed(e.to_string()))?;
    let text = String::from_utf8_lossy(chunk);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\r' => line_buf.clear(),
            '\n' => flush_logged_line(pty, line_buf, span, true)?,
            ch => line_buf.push(ch),
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
    span: &mut CommandSpanState,
) -> Result<(), InteractiveShellError> {
    loop {
        match master_file.read(buf) {
            Ok(0) => break,
            Ok(n) => relay_master_chunk(&buf[..n], stdout, pty, line_buf, span)?,
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

fn exit_code_from_status(status: libc::c_int) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status) as i32
    } else {
        1
    }
}

fn flush_logged_line<L: SessionLog>(
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
    span: &mut CommandSpanState,
    include_newline: bool,
) -> Result<(), InteractiveShellError> {
    if include_newline {
        if line_buf.is_empty() {
            pty.append_stdout("\n", span)?;
        } else {
            line_buf.push('\n');
            pty.append_stdout(line_buf, span)?;
            line_buf.clear();
        }
    } else if !line_buf.is_empty() {
        pty.append_stdout(line_buf, span)?;
        line_buf.clear();
    }
    Ok(())
}

fn flush_line<L: SessionLog>(
    pty: &mut PtyShell<'_, L>,
    line_buf: &mut String,
    span: &mut CommandSpanState,
) -> Result<(), InteractiveShellError> {
    flush_logged_line(pty, line_buf, span, false)
}

/// span 記録から PTY の入力行 echo を 1 行だけ除く。
fn strip_shell_echo_from_span_output(span: &mut CommandSpanState, data: &str) -> String {
    let Some(command) = span.strip_echo_line.as_ref() else {
        return data.to_string();
    };
    let (filtered, stripped) = strip_first_shell_echo_line(data, command);
    if stripped {
        span.strip_echo_line = None;
        return filtered;
    }
    filtered
}

fn extract_after_shell_echo(data: &str, command: &str) -> Option<String> {
    if command.is_empty() {
        return None;
    }
    let mut offset = 0;
    for line in data.split_inclusive('\n') {
        let display_line = line.trim_end_matches(['\r', '\n']);
        if line_looks_like_shell_echo(display_line, command) {
            return Some(data[offset + line.len()..].to_string());
        }
        offset += line.len();
    }
    None
}

fn split_before_shell_echo(data: &str, command: &str) -> Option<(String, String)> {
    if command.is_empty() {
        return None;
    }
    let mut offset = 0;
    for line in data.split_inclusive('\n') {
        let display_line = line.trim_end_matches(['\r', '\n']);
        if line_looks_like_shell_echo(display_line, command) {
            return Some((
                data[..offset].to_string(),
                data[offset + line.len()..].to_string(),
            ));
        }
        offset += line.len();
    }
    None
}

fn split_before_any_prompt_echo(data: &str) -> Option<(String, String)> {
    let mut offset = 0;
    for line in data.split_inclusive('\n') {
        let display_line = line.trim_end_matches(['\r', '\n']);
        if line_looks_like_any_prompt_echo(display_line) {
            return Some((
                data[..offset].to_string(),
                data[offset + line.len()..].to_string(),
            ));
        }
        offset += line.len();
    }
    None
}

/// 先頭の 1 行だけが shell の入力行 echo なら除く。
fn strip_first_shell_echo_line(data: &str, command: &str) -> (String, bool) {
    if command.is_empty() {
        return (data.to_string(), false);
    }
    let Some((first, rest)) = data.split_once('\n') else {
        if line_looks_like_shell_echo(data, command) {
            return (String::new(), true);
        }
        return (data.to_string(), false);
    };
    if line_looks_like_shell_echo(first, command) {
        return (rest.to_string(), true);
    }
    (data.to_string(), false)
}

fn line_looks_like_shell_echo(line: &str, command: &str) -> bool {
    let line = line.trim_end_matches('\r');
    if line == command {
        return true;
    }
    if !line.ends_with(command) {
        return false;
    }
    if line.len() == command.len() {
        return true;
    }
    let boundary = line.len() - command.len();
    matches!(
        line.as_bytes().get(boundary.wrapping_sub(1)),
        None | Some(b' ') | Some(b'\t') | Some(b'$')
    )
}

fn line_looks_like_any_prompt_echo(line: &str) -> bool {
    let line = line.trim_end_matches('\r');
    ['$', '#', '%', '>'].iter().any(|marker| {
        line.rsplit_once(*marker)
            .map(|(_, command)| !command.trim().is_empty())
            .unwrap_or(false)
    })
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

    /// 親が TTY（aish shell 等）でも、非 TTY 前提の単体テストを CI 相当にする。
    struct NonTtyStdinGuard {
        saved: RawFd,
    }

    impl NonTtyStdinGuard {
        fn install() -> Self {
            let saved = unsafe { libc::dup(libc::STDIN_FILENO) };
            assert!(saved >= 0, "dup(stdin)");
            let null =
                unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
            assert!(null >= 0, "open(/dev/null)");
            assert!(
                unsafe { libc::dup2(null, libc::STDIN_FILENO) } >= 0,
                "dup2(/dev/null -> stdin)"
            );
            unsafe {
                libc::close(null);
            }
            assert_eq!(
                unsafe { libc::isatty(libc::STDIN_FILENO) },
                0,
                "stdin must be non-tty for this test"
            );
            Self { saved }
        }
    }

    impl Drop for NonTtyStdinGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = libc::dup2(self.saved, libc::STDIN_FILENO);
                libc::close(self.saved);
            }
        }
    }

    #[test]
    fn winch_monitor_skips_install_when_stdin_not_tty() {
        let _stdin = NonTtyStdinGuard::install();
        assert!(WinchMonitor::install().is_err());
    }

    #[test]
    fn sync_pty_winsize_from_stdin_is_noop_when_stdin_has_no_size() {
        let (master, slave) = open_pty_pair().expect("openpty");
        unsafe {
            libc::close(slave);
        }
        sync_pty_winsize_from_stdin(master).expect("sync should not fail");
        close_raw_fd(master);
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
    fn pty_relay_cleanup_drop_terminates_and_reaps_child() {
        let (master, slave) = open_pty_pair().expect("openpty");
        let (ready_r, ready_w) = pipe_pair();
        let child = match unsafe { libc::fork() } {
            -1 => panic!("fork failed"),
            0 => {
                unsafe {
                    libc::close(master);
                    libc::close(ready_r);
                    // 本番の PTY shell と同様、独自 session に入る。
                    assert!(libc::setsid() >= 0, "setsid");
                    let _ = libc::ioctl(slave, libc::TIOCSCTTY, 0);
                    for stdfd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
                        let _ = libc::dup2(slave, stdfd);
                    }
                    libc::close(slave);
                    // SIGHUP/SIGTERM 無視でも Drop → SIGKILL escalation で回収できること。
                    libc::signal(libc::SIGHUP, libc::SIG_IGN);
                    libc::signal(libc::SIGTERM, libc::SIG_IGN);
                    let byte = 1u8;
                    let _ = libc::write(ready_w, (&raw const byte).cast(), 1);
                    libc::close(ready_w);
                    loop {
                        libc::sleep(1);
                    }
                }
            }
            pid => pid,
        };
        unsafe {
            libc::close(slave);
            libc::close(ready_w);
        }
        let mut ready = [0u8; 1];
        let n = unsafe { libc::read(ready_r, ready.as_mut_ptr().cast(), 1) };
        unsafe {
            libc::close(ready_r);
        }
        assert_eq!(n, 1, "child must signal setsid readiness");

        let master_file = unsafe { std::fs::File::from_raw_fd(master) };

        // 一般エラー経路相当: disarm せず Drop → terminate_pty_session。
        drop(PtyRelayCleanup::new(child, master_file));

        let mut status: libc::c_int = 0;
        let pid = unsafe { libc::waitpid(child, &mut status, libc::WNOHANG) };
        assert!(
            pid <= 0,
            "child should be reaped by PtyRelayCleanup drop (waitpid returned {pid})"
        );
    }

    #[test]
    fn pty_relay_cleanup_mark_reaped_does_not_kill() {
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
        let master_file = unsafe { std::fs::File::from_raw_fd(master) };
        let mut cleanup = PtyRelayCleanup::new(child, master_file);
        cleanup.mark_child_reaped();
        drop(cleanup);

        // mark_child_reaped 後は terminate しないので子は生存したまま。
        let alive = unsafe { libc::kill(child, 0) } == 0;
        assert!(
            alive,
            "child must still be alive after mark_child_reaped drop"
        );

        unsafe {
            let _ = libc::kill(child, libc::SIGKILL);
            let mut status = 0;
            let _ = libc::waitpid(child, &mut status, 0);
        }
    }

    #[test]
    fn shell_command_span_records_index_and_timestamps() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
        let mut pty = PtyShell::new(&mut log);
        let mut span = CommandSpanState {
            next_index: 0,
            active_index: None,
            replay_enabled: true,
            ..CommandSpanState::default()
        };
        pty.handle_control_line(r#"{"event":"start","command":"echo hi"}"#, &mut span)
            .expect("start");
        assert_eq!(span.active_index, Some(1));
        pty.append_stdout("hi", &mut span).expect("stdout");
        pty.handle_control_line(r#"{"event":"end","exit_code":0}"#, &mut span)
            .expect("end");
        pty.finish_active_span(&mut span).expect("finish");
        assert!(span.active_index.is_none());

        let content = std::fs::read_to_string(log_path).expect("read");
        assert!(content.contains(r#""event":"command_start""#));
        assert!(content.contains(r#""command_index":1"#));
        assert!(content.contains(r#""event":"command_end""#));
        assert!(content.contains("hi"));
    }

    #[test]
    fn shell_command_span_strips_prompt_echo_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
        let mut pty = PtyShell::new(&mut log);
        let mut span = CommandSpanState {
            next_index: 0,
            active_index: None,
            replay_enabled: true,
            ..CommandSpanState::default()
        };
        pty.handle_control_line(r#"{"event":"start","command":"ls"}"#, &mut span)
            .expect("start");
        pty.append_stdout("honda@host:~/labo/aish$ ls\n", &mut span)
            .expect("echo");
        pty.append_stdout("AGENTS.md\n", &mut span).expect("stdout");
        pty.handle_control_line(r#"{"event":"end","exit_code":0}"#, &mut span)
            .expect("end");
        pty.finish_active_span(&mut span).expect("finish");

        let content = std::fs::read_to_string(log_path).expect("read");
        assert!(
            !content.contains("honda@host"),
            "prompt echo must not be logged: {content}"
        );
        assert!(content.contains("AGENTS.md"));
    }

    #[test]
    fn replay_show_excludes_unindexed_shell_output_before_start() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        {
            let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
            let mut pty = PtyShell::new(&mut log);
            let mut span = CommandSpanState {
                next_index: 0,
                active_index: None,
                replay_enabled: true,
                ..CommandSpanState::default()
            };
            pty.append_stdout("startup banner\nhonda@host:~/aish$ ", &mut span)
                .expect("startup");
            pty.handle_control_line(r#"{"event":"start","command":"printf ok"}"#, &mut span)
                .expect("start");
            pty.append_stdout("printf ok\n", &mut span).expect("echo");
            pty.append_stdout("ok\n", &mut span).expect("stdout");
            pty.handle_control_line(r#"{"event":"end","exit_code":0}"#, &mut span)
                .expect("end");
            pty.finish_active_span(&mut span).expect("finish");
        }

        let events = crate::adapters::outbound::read_log_events(&log_path).expect("events");
        let out = crate::application::replay_show(&events, 1, false).expect("show");
        assert_eq!(out, "ok\n");
    }

    #[test]
    fn replay_show_keeps_fast_output_that_arrives_before_start_control() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        {
            let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
            let mut pty = PtyShell::new(&mut log);
            let mut span = CommandSpanState {
                next_index: 0,
                active_index: None,
                replay_enabled: true,
                ..CommandSpanState::default()
            };
            pty.append_stdout("startup\nhonda@host:~/aish$ printf ok\nok\n", &mut span)
                .expect("pending");
            pty.handle_control_line(r#"{"event":"start","command":"printf ok"}"#, &mut span)
                .expect("start");
            pty.handle_control_line(r#"{"event":"end","exit_code":0}"#, &mut span)
                .expect("end");
            pty.finish_active_span(&mut span).expect("finish");
        }

        let events = crate::adapters::outbound::read_log_events(&log_path).expect("events");
        let out = crate::application::replay_show(&events, 1, false).expect("show");
        assert_eq!(out, "ok\n");
    }

    #[test]
    fn relay_master_chunk_preserves_newlines_in_log() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
        let mut pty = PtyShell::new(&mut log);
        let mut line_buf = String::new();
        let mut span = CommandSpanState {
            next_index: 1,
            active_index: Some(1),
            replay_enabled: true,
            ..CommandSpanState::default()
        };
        let mut stdout = std::io::stdout().lock();
        relay_master_chunk(
            b"line1\nline2\n",
            &mut stdout,
            &mut pty,
            &mut line_buf,
            &mut span,
        )
        .expect("relay");
        assert!(line_buf.is_empty());
        let content = std::fs::read_to_string(log_path).expect("read");
        assert!(content.contains(r#""data":"line1\n""#), "{content}");
        assert!(content.contains(r#""data":"line2\n""#), "{content}");
    }

    #[test]
    fn relay_master_chunk_logs_crlf_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log_path = dir.path().join("shell.jsonl");
        let mut log = crate::adapters::outbound::JsonlFileLog::new(log_path.clone());
        let mut pty = PtyShell::new(&mut log);
        let mut line_buf = String::new();
        let mut span = CommandSpanState {
            next_index: 1,
            active_index: Some(1),
            replay_enabled: true,
            ..CommandSpanState::default()
        };
        let mut stdout = std::io::stdout().lock();
        relay_master_chunk(
            b"AGENTS.md  Cargo.toml\r\nCargo.lock  LICENSE\r\n",
            &mut stdout,
            &mut pty,
            &mut line_buf,
            &mut span,
        )
        .expect("relay");
        let content = std::fs::read_to_string(log_path).expect("read");
        assert!(content.contains("AGENTS.md  Cargo.toml"), "{content}");
        assert!(content.contains("Cargo.lock  LICENSE"), "{content}");
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
    fn stdin_byte_eot_is_forwarded_to_pty() {
        let (input_read, input_write) = pipe_pair();
        let (shutdown_read, shutdown_write) = open_shutdown_pipe().expect("shutdown pipe");
        let (pty_read, pty_write) = pipe_pair();

        let relay_handle = thread::spawn(move || {
            relay_stdin_to_pty(input_read, pty_write, shutdown_read);
        });

        {
            let mut input_file = unsafe { std::fs::File::from_raw_fd(input_write) };
            input_file.write_all(&[0x04]).expect("write EOT");
        }
        close_raw_fd(input_write);

        relay_handle.join().expect("relay thread panicked");
        close_raw_fd(shutdown_write);

        let mut output = Vec::new();
        unsafe { std::fs::File::from_raw_fd(pty_read) }
            .read_to_end(&mut output)
            .expect("read pty output");
        assert_eq!(output, vec![0x04]);
    }

    #[test]
    fn stdin_eof_does_not_synthesize_eot() {
        let (input_read, input_write) = pipe_pair();
        let (shutdown_read, shutdown_write) = open_shutdown_pipe().expect("shutdown pipe");
        let (pty_read, pty_write) = pipe_pair();

        let relay_handle = thread::spawn(move || {
            relay_stdin_to_pty(input_read, pty_write, shutdown_read);
        });

        close_raw_fd(input_write);

        relay_handle.join().expect("relay thread panicked");
        close_raw_fd(shutdown_write);

        let mut output = Vec::new();
        unsafe { std::fs::File::from_raw_fd(pty_read) }
            .read_to_end(&mut output)
            .expect("read pty output");
        assert!(output.is_empty(), "EOF must not synthesize EOT");
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
