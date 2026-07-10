#![cfg(unix)]
//! 0057 PTY process cleanup hardening acceptance tests.

use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn e2e_timeout() -> Duration {
    Duration::from_secs(
        std::env::var("AISH_0057_E2E_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(12)
            .min(60),
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
    command.spawn().expect("spawn child")
}

fn kill_process_group_and_reap(child: Child) -> std::process::Output {
    let pgid = child.id() as i32;
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    child.wait_with_output().expect("wait after kill")
}

fn wait_child_with_timeout(mut child: Child, deadline: Instant) -> std::process::Output {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().expect("wait output"),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let output = kill_process_group_and_reap(child);
                panic!(
                    "aish human-shell timed out, stderr={}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => panic!("wait child: {e}"),
        }
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

fn wait_pid_gone_or_zombie(pid: i32, deadline: Instant) {
    while Instant::now() < deadline {
        if !pid_exists(pid) || process_state(pid) == Some('Z') {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("pid {pid} still alive with state {:?}", process_state(pid));
}

fn read_pid(path: &Path) -> i32 {
    fs::read_to_string(path)
        .expect("pid file")
        .trim()
        .parse()
        .expect("pid")
}

fn wait_for_pid_file(path: &Path, deadline: Instant) -> i32 {
    while Instant::now() < deadline {
        if path.is_file() {
            return read_pid(path);
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("pid file not written: {}", path.display());
}

fn spawn_human_shell(home: &Path, input: String) -> Child {
    let result_file = home.join("result.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_aish"));
    command
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home)
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "0057 cleanup")
        .env("AISH_HANDOFF_SUGGESTED_COMMAND", "true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_in_new_process_group(&mut command);
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write input");
    child
}

#[test]
fn aish_human_shell_and_pty_shell_reaped() {
    let home = tempfile::tempdir().expect("home");
    let shell_pid = home.path().join("shell.pid");
    let script = format!(
        "echo $$ > {}\nsleep 30\n",
        shell_quote(shell_pid.to_str().unwrap())
    );
    let deadline = Instant::now() + e2e_timeout();
    let child = spawn_human_shell(home.path(), script);
    let pty_shell = wait_for_pid_file(&shell_pid, deadline);
    thread::sleep(Duration::from_millis(300));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = wait_child_with_timeout(child, deadline);
    assert!(
        !output.status.success(),
        "SIGTERM cancel must not report success"
    );
    wait_pid_gone_or_zombie(pty_shell, deadline);
}

#[test]
fn foreground_and_normal_background_jobs_terminated() {
    let home = tempfile::tempdir().expect("home");
    let fg_pid = home.path().join("fg.pid");
    let bg_pid = home.path().join("bg.pid");
    let script = format!(
        "sleep 30 & echo $! > {}\nsh -c 'echo $$ > {}; sleep 30'\n",
        shell_quote(bg_pid.to_str().unwrap()),
        shell_quote(fg_pid.to_str().unwrap()),
    );
    let deadline = Instant::now() + e2e_timeout();
    let child = spawn_human_shell(home.path(), script);
    let bg = wait_for_pid_file(&bg_pid, deadline);
    let fg = wait_for_pid_file(&fg_pid, deadline);
    thread::sleep(Duration::from_millis(300));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = wait_child_with_timeout(child, deadline);
    assert!(
        !output.status.success(),
        "SIGTERM cancel must not report success"
    );
    wait_pid_gone_or_zombie(fg, deadline);
    wait_pid_gone_or_zombie(bg, deadline);
}

#[test]
fn sigterm_ignored_escalates_sigkill() {
    let home = tempfile::tempdir().expect("home");
    let stubborn_pid = home.path().join("stubborn.pid");
    let script = format!(
        "sh -c 'trap \"\" TERM HUP; echo $$ > {}; while :; do sleep 1; done'\n",
        shell_quote(stubborn_pid.to_str().unwrap()),
    );
    let deadline = Instant::now() + e2e_timeout();
    let child = spawn_human_shell(home.path(), script);
    let stubborn = wait_for_pid_file(&stubborn_pid, deadline);
    thread::sleep(Duration::from_millis(300));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = wait_child_with_timeout(child, deadline);
    assert!(
        !output.status.success(),
        "SIGTERM cancel must not report success"
    );
    wait_pid_gone_or_zombie(stubborn, deadline);
}

#[test]
fn direct_children_reaped_no_zombies() {
    let home = tempfile::tempdir().expect("home");
    let shell_pid = home.path().join("shell.pid");
    let script = format!(
        "echo $$ > {}\nsleep 30\n",
        shell_quote(shell_pid.to_str().unwrap())
    );
    let deadline = Instant::now() + e2e_timeout();
    let child = spawn_human_shell(home.path(), script);
    let aish_pid = child.id() as i32;
    let pty_shell = wait_for_pid_file(&shell_pid, deadline);
    thread::sleep(Duration::from_millis(300));
    unsafe {
        libc::kill(aish_pid, libc::SIGTERM);
    }
    let output = wait_child_with_timeout(child, deadline);
    assert!(
        !output.status.success(),
        "SIGTERM cancel must not report success"
    );
    wait_pid_gone_or_zombie(pty_shell, deadline);
    assert!(
        !pid_exists(aish_pid) || process_state(aish_pid) != Some('Z'),
        "aish direct child process must not remain zombie"
    );
}
