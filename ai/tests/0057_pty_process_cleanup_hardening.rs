#![cfg(unix)]
//! 0057 PTY process cleanup hardening acceptance tests.
//!
//! - launcher 単体（fake aish + cancel flag）
//! - real vertical E2E: mock aibe + `ai --collaborative --timeout` / OS signal
//!   + real `aish human-shell` + PTY

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ai::adapters::outbound::{AishHumanShellLauncher, RuntimeHandoffDirGuard};
use ai::ports::outbound::{HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher};
use aibe_protocol::{ClientRequest, ClientResponse, ErrorCode, ShellExecApprovalOrigin};

fn e2e_timeout() -> Duration {
    let secs = std::env::var("AISH_0057_E2E_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    Duration::from_secs(secs.min(60))
}

fn ai_timeout_secs() -> u64 {
    std::env::var("AISH_0057_AI_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
        .clamp(1, 5)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn make_executable(path: &Path) {
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).unwrap();
}

fn fake_aish_script(home: &Path, body: &str) -> PathBuf {
    let path = home.join("fake-aish");
    fs::write(
        &path,
        format!(
            r#"#!/bin/sh
set -eu
if [ "${{1:-}}" != "human-shell" ] || [ "${{2:-}}" != "--result-file" ]; then
  echo "unexpected fake aish args: $*" >&2
  exit 64
fi
result_file="$3"
{}
"#,
            body
        ),
    )
    .unwrap();
    make_executable(&path);
    path
}

fn request(cwd: &Path, runtime_dir: PathBuf) -> HumanShellLaunchRequest {
    HumanShellLaunchRequest {
        cwd: cwd.to_path_buf(),
        parent_request_summary: "0057 parent".into(),
        suggested_command: "true".into(),
        runtime_dir,
    }
}

fn assert_no_runtime_handoff_dirs(root: &Path) {
    if !root.is_dir() {
        return;
    }
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        assert!(
            !name.starts_with("handoff-"),
            "runtime handoff dir leaked: {}",
            entry.path().display()
        );
    }
}

fn pid_exists(pid: i32) -> bool {
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }
}

fn process_state(pid: i32) -> Option<char> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rp = stat.rfind(')')?;
    stat[rp + 2..].chars().next()
}

/// vertical E2E 用: zombie 残留は成功にせず、期限まで reap を待って PID 完全消滅を要求する。
fn wait_pid_fully_gone(pid: i32, deadline: Instant) {
    while Instant::now() < deadline {
        if !pid_exists(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!(
        "pid {pid} was not fully reaped; final state={:?}",
        process_state(pid)
    );
}

fn read_pid_file(path: &Path) -> Option<i32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn spawn_in_new_process_group(command: &mut Command) -> Child {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().expect("spawn child in new process group")
}

fn kill_process_group_and_reap(child: Child) -> std::process::Output {
    let pgid = child.id() as i32;
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    child
        .wait_with_output()
        .expect("wait output after process group kill")
}

fn wait_child_with_timeout(mut child: Child, deadline: Instant) -> std::process::Output {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().expect("wait output"),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let output = kill_process_group_and_reap(child);
                panic!(
                    "ai child timed out after {:?}, stderr={}",
                    e2e_timeout(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => panic!("wait child failed: {e}"),
        }
    }
}

fn accept_with_deadline(
    listener: &UnixListener,
    deadline: Instant,
) -> (
    std::os::unix::net::UnixStream,
    std::os::unix::net::SocketAddr,
) {
    loop {
        if Instant::now() >= deadline {
            panic!("mock server accept deadline exceeded");
        }
        match listener.accept() {
            Ok(conn) => return conn,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("mock server accept failed: {e}"),
        }
    }
}

fn read_line_with_deadline(
    reader: &mut BufReader<std::os::unix::net::UnixStream>,
    line: &mut String,
    deadline: Instant,
) -> std::io::Result<()> {
    while Instant::now() < deadline {
        reader.get_mut().set_nonblocking(true)?;
        match reader.read_line(line) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "mock server connection closed by peer",
                ));
            }
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "mock server read_line deadline exceeded",
    ))
}

fn join_with_deadline(handle: JoinHandle<()>, deadline: Instant) {
    while Instant::now() < deadline {
        if handle.is_finished() {
            handle.join().expect("mock aibe server panicked");
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("mock server thread join deadline exceeded");
}

fn write_ai_config(home: &Path, socket_path: &Path, history_dir: &Path) -> PathBuf {
    let path = home.join("ai.toml");
    fs::write(
        &path,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
history_max_entries = 0
[ask]
tools = "shell_exec"
progress = false
"#,
            socket_path.display(),
            history_dir.display(),
        ),
    )
    .expect("write ai config");
    path
}

fn write_aibe_config(home: &Path) -> PathBuf {
    let path = home.join("aibe.toml");
    fs::write(
        &path,
        r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#,
    )
    .expect("write aibe config");
    path
}

fn write_aish_config(home: &Path, log_dir: &Path) -> PathBuf {
    let path = home.join("aish.toml");
    fs::write(
        &path,
        format!(
            "log_dir = \"{}\"\n",
            log_dir.display().to_string().replace('\\', "\\\\")
        ),
    )
    .expect("write aish config");
    path
}

fn require_aish_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_aish") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "CARGO_BIN_EXE_aish points to missing file: {}",
            path.display()
        );
        return path;
    }
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let sibling = ai_bin
        .parent()
        .map(|dir| dir.join("aish"))
        .unwrap_or_else(|| PathBuf::from("aish"));
    assert!(
        sibling.is_file(),
        "aish binary not found next to ai (set CARGO_BIN_EXE_aish or build aish): {}",
        sibling.display()
    );
    sibling
}

/// mock aibe: AgentTurn → ShellExecApprovalPrompt →（cancel 後の）failed approval → Error。
/// 同一 listener で cancel_turn 接続も受け、AgentTurn 以外は捨てる。
struct CancelPathHandoffMock {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
    saw_cancel_turn: Arc<AtomicBool>,
}

impl CancelPathHandoffMock {
    fn spawn() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        listener.set_nonblocking(true).expect("set_nonblocking");
        let saw_cancel_turn = Arc::new(AtomicBool::new(false));
        let saw_cancel_turn_thread = Arc::clone(&saw_cancel_turn);

        let deadline = Instant::now() + e2e_timeout();
        let handle = thread::spawn(move || {
            let (stream, first_line) = loop {
                if Instant::now() >= deadline {
                    panic!("mock server did not receive agent_turn before deadline");
                }
                let (stream, _) = accept_with_deadline(&listener, deadline);
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                match read_line_with_deadline(&mut reader, &mut line, deadline) {
                    Ok(()) => match serde_json::from_str::<ClientRequest>(line.trim()) {
                        Ok(ClientRequest::AgentTurn { .. }) => {
                            // clone 側で読んだので共有 socket から消費済み。clone の reader を使う。
                            drop(stream);
                            break (reader, line);
                        }
                        Ok(ClientRequest::CancelTurn { .. }) => {
                            saw_cancel_turn_thread.store(true, Ordering::SeqCst);
                            continue;
                        }
                        Ok(_) | Err(_) => continue,
                    },
                    Err(_) => continue,
                }
            };
            let mut reader = stream;
            let mut writer = reader.get_mut().try_clone().expect("clone writer");
            let line = first_line;
            let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse agent_turn");
            let ClientRequest::AgentTurn {
                id,
                context,
                messages,
                ..
            } = req
            else {
                panic!("expected agent_turn");
            };
            assert!(
                context.collaborative_handoff,
                "collaborative_handoff must be true"
            );
            assert!(
                messages.iter().any(|m| m.role == "user"),
                "user message required"
            );

            let prompt = ClientResponse::ShellExecApprovalPrompt {
                id: "handoff-prompt-0057".into(),
                turn_id: id.clone(),
                tool_call_id: "call_handoff_0057".into(),
                command: "true".into(),
                args: vec![],
            };
            let prompt_json = serde_json::to_string(&prompt).expect("prompt json");
            writeln!(writer, "{prompt_json}").expect("write prompt");
            writer.flush().expect("flush");

            // approval 待ち中も cancel 接続を受け付ける
            let listener_cancel = listener.try_clone().expect("clone listener for cancel");
            let stop_cancel = Arc::new(AtomicBool::new(false));
            let stop_cancel_thread = Arc::clone(&stop_cancel);
            let saw_cancel_drain = Arc::clone(&saw_cancel_turn_thread);
            let cancel_deadline = deadline;
            let cancel_drain = thread::spawn(move || {
                while Instant::now() < cancel_deadline && !stop_cancel_thread.load(Ordering::SeqCst)
                {
                    match listener_cancel.accept() {
                        Ok((stream, _)) => {
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            if read_line_with_deadline(&mut reader, &mut line, cancel_deadline)
                                .is_ok()
                            {
                                if matches!(
                                    serde_json::from_str::<ClientRequest>(line.trim()),
                                    Ok(ClientRequest::CancelTurn { .. })
                                ) {
                                    saw_cancel_drain.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            });

            let mut approval_line = String::new();
            read_line_with_deadline(&mut reader, &mut approval_line, deadline)
                .expect("read shell_exec_approval after cancel");
            stop_cancel.store(true, Ordering::SeqCst);
            let _ = cancel_drain.join();

            let approval: ClientRequest =
                serde_json::from_str(approval_line.trim()).expect("parse approval");
            let ClientRequest::ShellExecApproval {
                approved,
                approval_origin,
                handoff_result,
                handoff_error,
                ..
            } = approval
            else {
                panic!(
                    "expected shell_exec_approval, got: {}",
                    approval_line.trim()
                );
            };
            assert!(!approved, "cancel path must not approve handoff as success");
            assert_eq!(
                approval_origin,
                ShellExecApprovalOrigin::CollaborativeHandoff
            );
            assert!(handoff_result.is_none());
            assert!(handoff_error.is_some(), "cancel must report handoff_error");

            let final_resp = ClientResponse::Error {
                id,
                code: ErrorCode::ToolError,
                message: handoff_error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "human_handoff_failed".into()),
            };
            let final_json = serde_json::to_string(&final_resp).expect("final json");
            writeln!(writer, "{final_json}").expect("write final");
            writer.flush().expect("flush");
        });

        Self {
            socket_path,
            _dir: dir,
            handle: Some(handle),
            saw_cancel_turn,
        }
    }

    fn join(mut self, deadline: Instant) -> bool {
        if let Some(handle) = self.handle.take() {
            join_with_deadline(handle, deadline);
        }
        self.saw_cancel_turn.load(Ordering::SeqCst)
    }
}

fn wait_for_handoff_pids(
    child: &mut Child,
    pty_pid_file: &Path,
    aish_pid_file: &Path,
    job_pid_file: &Path,
    deadline: Instant,
) -> Option<(i32, i32, i32)> {
    while Instant::now() < deadline {
        if let (Some(pty), Some(aish_pid), Some(job)) = (
            read_pid_file(pty_pid_file),
            read_pid_file(aish_pid_file),
            read_pid_file(job_pid_file),
        ) {
            return Some((pty, aish_pid, job));
        }
        match child.try_wait() {
            Ok(Some(_)) => return None,
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("try_wait failed: {e}"),
        }
    }
    None
}

/// launcher 単体: 既に立っている cancel flag で子を止められること。
#[test]
fn launcher_cancel_flag_stops_child_bounded() {
    let home = tempfile::tempdir().unwrap();
    let runtime = home.path().join("aish").join("handoff-timeout");
    let fake = fake_aish_script(home.path(), "sleep 30");
    let launcher = AishHumanShellLauncher::new(fake);
    // 単体: cancel 済み flag を渡す（vertical E2E は --timeout 実経路を使う）
    let cancel = AtomicBool::new(false);
    cancel.store(true, Ordering::SeqCst);
    let started = Instant::now();
    let err = launcher
        .launch_and_wait(&request(home.path(), runtime), &cancel)
        .expect_err("cancelled handoff must fail");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancelled handoff must be bounded"
    );
    assert!(matches!(err, HumanShellLaunchError::Cancelled(_)));
}

/// AC: `ai --collaborative --timeout` の実経路で handoff 中でも有限時間 non-zero 終了する。
#[test]
fn handoff_timeout_terminates_bounded() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let aish_bin = require_aish_binary();
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let aish_log = home.path().join("aish-sessions");
    fs::create_dir_all(&aish_log).expect("aish log");

    let pty_pid_file = work.join("pty.pid");
    let aish_pid_file = work.join("aish.pid");
    let job_pid_file = work.join("job.pid");

    let server = CancelPathHandoffMock::spawn();
    let ai_cfg = write_ai_config(home.path(), &server.socket_path, &history_dir);
    let aibe_cfg = write_aibe_config(home.path());
    let aish_cfg = write_aish_config(home.path(), &aish_log);

    let timeout_secs = ai_timeout_secs();
    let deadline = Instant::now() + e2e_timeout();
    let started = Instant::now();

    let shell_input = format!(
        "echo $$ > {}\necho $PPID > {}\nsleep 30 &\necho $! > {}\nsleep 30\n",
        shell_quote(pty_pid_file.to_str().unwrap()),
        shell_quote(aish_pid_file.to_str().unwrap()),
        shell_quote(job_pid_file.to_str().unwrap()),
    );

    let mut child = {
        let mut command = Command::new(&ai_bin);
        command
            .args([
                "ask",
                "--collaborative",
                "--quiet",
                "--no-start",
                "--timeout",
                &timeout_secs.to_string(),
                "verify 0057 timeout handoff cleanup",
            ])
            .current_dir(&work)
            .env("AI_CONFIG", &ai_cfg)
            .env("AIBE_CONFIG", &aibe_cfg)
            .env("AISH_CONFIG", &aish_cfg)
            .env("HOME", home.path())
            .env("AISH_BIN", &aish_bin)
            .env("XDG_RUNTIME_DIR", home.path())
            .env("SHELL", "/bin/bash")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        spawn_in_new_process_group(&mut command)
    };

    // stdin を閉じると shell が EOF で終わるため、timeout まで開いたままにする。
    let mut stdin = child.stdin.take().expect("ai stdin");
    stdin
        .write_all(shell_input.as_bytes())
        .expect("write shell input");
    stdin.flush().expect("flush shell input");

    let captured = wait_for_handoff_pids(
        &mut child,
        &pty_pid_file,
        &aish_pid_file,
        &job_pid_file,
        deadline,
    );

    let output = wait_child_with_timeout(child, deadline);
    drop(stdin);
    let saw_cancel = server.join(deadline);

    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        elapsed < e2e_timeout(),
        "ai must finish before outer watchdog ({:?}); elapsed={elapsed:?}; stderr={stderr}",
        e2e_timeout()
    );
    assert!(
        elapsed < Duration::from_secs(timeout_secs + 8),
        "ai should exit soon after --timeout {timeout_secs}s (plus cleanup); elapsed={elapsed:?}; stderr={stderr}"
    );
    assert!(
        !output.status.success(),
        "timeout during handoff must not be a normal success; code={:?}; stderr={stderr}",
        output.status.code()
    );
    assert!(
        saw_cancel,
        "timeout path must send cancel_turn to mock aibe"
    );

    let (pty_pid, aish_pid, job_pid) = captured.unwrap_or_else(|| {
        panic!(
            "handoff shell never wrote pid files before ai exited; \
             timeout may have fired before PTY was ready. stderr={stderr}"
        );
    });
    wait_pid_fully_gone(pty_pid, deadline);
    wait_pid_fully_gone(aish_pid, deadline);
    wait_pid_fully_gone(job_pid, deadline);

    let runtime_root = home.path().join("aish");
    assert_no_runtime_handoff_dirs(&runtime_root);
}

/// launcher 単体: 遅延で立った cancel flag を検知できること（OS signal は送らない）。
#[test]
fn launcher_delayed_cancel_flag_stops_child_bounded() {
    let home = tempfile::tempdir().unwrap();
    let runtime = home.path().join("aish").join("handoff-signal");
    let fake = fake_aish_script(home.path(), "sleep 30");
    let launcher = AishHumanShellLauncher::new(fake);
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            cancel.store(true, Ordering::SeqCst);
        });
        let err = launcher
            .launch_and_wait(&request(home.path(), runtime), &cancel)
            .expect_err("signal cancel must fail");
        assert!(matches!(err, HumanShellLaunchError::Cancelled(_)));
    });
}

/// AC: 子プロセスの real `ai` へ SIGINT / SIGTERM を送り、handoff 中でも有限時間で停止する。
#[test]
fn external_sigint_sigterm_stops_handoff() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let signal_name = if signal == libc::SIGINT {
            "SIGINT"
        } else {
            "SIGTERM"
        };
        let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
        let aish_bin = require_aish_binary();
        let home = tempfile::tempdir().expect("home");
        let work = home.path().join("work");
        fs::create_dir_all(&work).expect("work");
        let history_dir = home.path().join("history");
        fs::create_dir_all(&history_dir).expect("history");
        let aish_log = home.path().join("aish-sessions");
        fs::create_dir_all(&aish_log).expect("aish log");

        let pty_pid_file = work.join(format!("pty-{signal_name}.pid"));
        let aish_pid_file = work.join(format!("aish-{signal_name}.pid"));
        let job_pid_file = work.join(format!("job-{signal_name}.pid"));

        let server = CancelPathHandoffMock::spawn();
        let ai_cfg = write_ai_config(home.path(), &server.socket_path, &history_dir);
        let aibe_cfg = write_aibe_config(home.path());
        let aish_cfg = write_aish_config(home.path(), &aish_log);

        let deadline = Instant::now() + e2e_timeout();
        let started = Instant::now();

        let shell_input = format!(
            "echo $$ > {}\necho $PPID > {}\nsleep 30 &\necho $! > {}\nsleep 30\n",
            shell_quote(pty_pid_file.to_str().unwrap()),
            shell_quote(aish_pid_file.to_str().unwrap()),
            shell_quote(job_pid_file.to_str().unwrap()),
        );

        let mut child = {
            let mut command = Command::new(&ai_bin);
            command
                .args([
                    "ask",
                    "--collaborative",
                    "--quiet",
                    "--no-start",
                    &format!("verify 0057 {signal_name} handoff cleanup"),
                ])
                .current_dir(&work)
                .env("AI_CONFIG", &ai_cfg)
                .env("AIBE_CONFIG", &aibe_cfg)
                .env("AISH_CONFIG", &aish_cfg)
                .env("HOME", home.path())
                .env("AISH_BIN", &aish_bin)
                .env("XDG_RUNTIME_DIR", home.path())
                .env("SHELL", "/bin/bash")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            spawn_in_new_process_group(&mut command)
        };
        let ai_pid = child.id() as i32;
        // stdin を閉じると shell が EOF で終わるため、signal 後まで開いたままにする。
        let mut stdin = child.stdin.take().expect("ai stdin");
        stdin
            .write_all(shell_input.as_bytes())
            .expect("write shell input");
        stdin.flush().expect("flush shell input");

        let captured = wait_for_handoff_pids(
            &mut child,
            &pty_pid_file,
            &aish_pid_file,
            &job_pid_file,
            deadline,
        );
        let (pty_pid, aish_pid, job_pid) = match captured {
            Some(pids) => pids,
            None => {
                let output = kill_process_group_and_reap(child);
                panic!(
                    "{signal_name}: handoff PIDs not ready before signal; stderr={}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        };

        // ai 自身の handler 経路を検証するため、PID 単体へ送る（process group ではない）。
        let kill_rc = unsafe { libc::kill(ai_pid, signal) };
        assert_eq!(
            kill_rc,
            0,
            "{signal_name}: kill(ai_pid={ai_pid}) failed: {}",
            std::io::Error::last_os_error()
        );

        let output = wait_child_with_timeout(child, deadline);
        drop(stdin);
        let saw_cancel = server.join(deadline);

        let elapsed = started.elapsed();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            elapsed < e2e_timeout(),
            "{signal_name}: ai must finish before outer watchdog ({:?}); elapsed={elapsed:?}; stderr={stderr}",
            e2e_timeout()
        );
        assert!(
            !output.status.success(),
            "{signal_name}: must not exit as normal success; code={:?}; stderr={stderr}",
            output.status.code()
        );
        assert!(
            saw_cancel,
            "{signal_name}: mock aibe must observe cancel_turn (or equivalent cancel connect)"
        );

        wait_pid_fully_gone(pty_pid, deadline);
        wait_pid_fully_gone(aish_pid, deadline);
        wait_pid_fully_gone(job_pid, deadline);
        assert_no_runtime_handoff_dirs(&home.path().join("aish"));
    }
}

#[test]
fn terminal_echo_canonical_restored() {
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
        "openpty failed"
    );
    let saved_stdin = unsafe { libc::dup(libc::STDIN_FILENO) };
    assert!(saved_stdin >= 0, "dup stdin");
    let slave_file = unsafe { fs::File::from_raw_fd(slave) };
    unsafe {
        libc::dup2(slave_file.as_raw_fd(), libc::STDIN_FILENO);
    }

    let mut original: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) },
        0
    );
    original.c_lflag |= libc::ECHO | libc::ICANON;
    assert_eq!(
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original) },
        0
    );

    {
        let _guard = ai::adapters::outbound::ParentTermiosGuard::save();
        let mut raw = original;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        assert_eq!(
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) },
            0
        );
    }

    let mut restored: libc::termios = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut restored) },
        0
    );
    unsafe {
        libc::dup2(saved_stdin, libc::STDIN_FILENO);
        libc::close(saved_stdin);
        libc::close(master);
    }
    assert_ne!(restored.c_lflag & libc::ECHO, 0, "ECHO restored");
    assert_ne!(restored.c_lflag & libc::ICANON, 0, "ICANON restored");
}

#[test]
fn runtime_handoff_dir_removed_on_abort() {
    let home = tempfile::tempdir().unwrap();
    let runtime_root = home.path().join("aish");
    let runtime_dir = runtime_root.join("handoff-abort");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(runtime_dir.join("partial"), b"incomplete").unwrap();
    {
        let _guard = RuntimeHandoffDirGuard::new(runtime_dir);
    }
    assert_no_runtime_handoff_dirs(&runtime_root);
}

#[test]
fn normal_handoff_success_path_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let runtime = home.path().join("aish").join("handoff-success");
    let body = format!(
        "cat > \"$result_file\" <<'JSON'\n{}\nJSON\n",
        serde_json::json!({
            "normal_return": true,
            "exit_code": 0,
            "final_cwd": home.path(),
            "shell_session_id": "sess-0057",
            "shell_session_dir": home.path().join("session"),
            "shell_log_start": 0,
            "shell_log_end": 0
        })
    );
    let fake = fake_aish_script(home.path(), &body);
    let launcher = AishHumanShellLauncher::new(fake);
    let cancel = AtomicBool::new(false);
    let returned = launcher
        .launch_and_wait(&request(home.path(), runtime), &cancel)
        .expect("normal handoff succeeds");
    assert!(returned.normal_return);
    assert_eq!(returned.exit_code, Some(0));
    assert_eq!(returned.final_cwd, home.path());
}

#[test]
fn cleanup_e2e_has_outer_watchdog() {
    let ai_source = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/0057_pty_process_cleanup_hardening.rs"
    ))
    .expect("read ai test source");
    let aish_source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("aish/tests/0057_pty_process_cleanup_hardening.rs"),
    )
    .expect("read aish test source");
    assert!(
        ai_source.contains("fn wait_child_with_timeout"),
        "ai E2E must use outer wait_child_with_timeout"
    );
    assert!(
        ai_source.contains("fn kill_process_group_and_reap"),
        "ai E2E must SIGKILL process group on watchdog expiry"
    );
    assert!(ai_source.contains("libc::SIGKILL"));
    assert!(ai_source.contains("fn e2e_timeout"));
    assert!(
        ai_source.contains("--timeout"),
        "vertical E2E must exercise real ai --timeout"
    );
    assert!(
        ai_source.contains("CARGO_BIN_EXE_ai"),
        "vertical E2E must launch real ai binary"
    );
    assert!(
        ai_source.contains("verify 0057 timeout handoff cleanup"),
        "vertical E2E must run a real collaborative ask prompt"
    );
    assert!(
        ai_source.contains("libc::kill(ai_pid, signal)"),
        "signal E2E must kill ai PID (not process group) with OS signal"
    );
    assert!(
        ai_source.contains("saw_cancel"),
        "vertical E2E must observe cancel_turn on mock aibe"
    );
    assert!(
        ai_source.contains("fn wait_pid_fully_gone"),
        "vertical E2E must require fully reaped PIDs (not zombie-as-success)"
    );

    let timeout_e2e = ai_source
        .split("fn handoff_timeout_terminates_bounded()")
        .nth(1)
        .and_then(|rest| {
            rest.split("fn launcher_delayed_cancel_flag_stops_child_bounded()")
                .next()
        })
        .expect("locate handoff_timeout_terminates_bounded body");
    assert!(
        !timeout_e2e.contains("cancel.store(true")
            && !timeout_e2e.contains("AtomicBool::new(true)"),
        "timeout vertical AC must not pre-set cancel flag; use --timeout path"
    );
    assert!(
        timeout_e2e.contains("saw_cancel")
            && timeout_e2e.contains("timeout path must send cancel_turn"),
        "timeout E2E must assert CancelTurn on mock aibe"
    );
    assert!(
        timeout_e2e.contains("wait_pid_fully_gone"),
        "timeout E2E must require fully reaped descendant PIDs"
    );

    let signal_e2e = ai_source
        .split("fn external_sigint_sigterm_stops_handoff()")
        .nth(1)
        .and_then(|rest| rest.split("fn terminal_echo_canonical_restored()").next())
        .expect("locate external_sigint_sigterm_stops_handoff body");
    assert!(
        !signal_e2e.contains("cancel.store(true"),
        "signal vertical AC must not pre-set cancel flag; use OS signal path"
    );
    assert!(
        signal_e2e.contains("libc::SIGINT") && signal_e2e.contains("libc::SIGTERM"),
        "signal E2E must cover both SIGINT and SIGTERM"
    );
    assert!(
        signal_e2e.contains("wait_pid_fully_gone"),
        "signal E2E must require fully reaped descendant PIDs"
    );

    assert!(aish_source.contains("fn wait_child_with_timeout"));
    assert!(aish_source.contains("fn kill_process_group_and_reap"));
    assert!(aish_source.contains("libc::SIGKILL"));
    assert!(aish_source.contains("e2e_timeout()"));
}
