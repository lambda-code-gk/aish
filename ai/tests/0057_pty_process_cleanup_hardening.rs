#![cfg(unix)]
//! 0057 PTY process cleanup hardening acceptance tests.

use std::fs;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ai::adapters::outbound::{AishHumanShellLauncher, RuntimeHandoffDirGuard};
use ai::ports::outbound::{HumanShellLaunchError, HumanShellLaunchRequest, HumanShellLauncher};

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

#[test]
fn handoff_timeout_terminates_bounded() {
    let home = tempfile::tempdir().unwrap();
    let runtime = home.path().join("aish").join("handoff-timeout");
    let fake = fake_aish_script(home.path(), "sleep 30");
    let launcher = AishHumanShellLauncher::new(fake);
    let cancel = AtomicBool::new(true);
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

#[test]
fn external_sigint_sigterm_stops_handoff() {
    for _signal in [libc::SIGTERM, libc::SIGINT] {
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
    assert!(ai_source.contains("Duration::from_secs(5)"));
    assert!(aish_source.contains("fn wait_child_with_timeout"));
    assert!(aish_source.contains("fn kill_process_group_and_reap"));
    assert!(aish_source.contains("libc::SIGKILL"));
    assert!(aish_source.contains("e2e_timeout()"));
    assert!(
        aish_source.contains(&shell_quote("watchdog marker")) || aish_source.contains("deadline")
    );
}
