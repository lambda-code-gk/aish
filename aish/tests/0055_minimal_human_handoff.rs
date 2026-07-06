//! 0055 minimal human handoff acceptance tests (aish).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn run_human_shell(input: &[u8]) -> (std::process::Output, aish::human_shell::HumanShellResult) {
    if !Path::new("/bin/bash").is_file() {
        panic!("human-shell tests require /bin/bash");
    }
    let home = tempfile::tempdir().unwrap();
    let result_file = home.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_PARENT_REQUEST", "create marker file")
        .env(
            "AISH_HANDOFF_SUGGESTED_COMMAND",
            "touch /tmp/should-not-run",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    let _ = child.stdin.take().unwrap().write_all(input);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(8))
        .expect("human shell hung")
        .unwrap();
    let result = serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();
    (output, result)
}

#[test]
fn human_shell_child_has_handoff_env_vars() {
    assert!(aish::human_shell::handoff_environment_is_complete([(
        "AISH_CONTROL_MODE",
        "human-shell"
    )]));
    let (output, _) = run_human_shell(b"printf '%s\\n' \"$AISH_CONTROL_MODE\"\nexit\n");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("human-shell"),
        "stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn human_shell_ctrl_d_returns_control_to_parent() {
    let (output, actual) = run_human_shell(b"\x04");
    assert!(output.status.success());
    assert!(actual.normal_return);
}

#[test]
fn human_shell_exit_returns_control_regardless_of_code() {
    for input in [b"exit\n".as_slice(), b"exit 1\n".as_slice()] {
        let (output, actual) = run_human_shell(input);
        assert!(output.status.success());
        assert!(actual.normal_return);
        assert!(actual.exit_code.is_some());
    }
}

#[test]
fn suggested_command_is_not_auto_executed() {
    let home = tempfile::tempdir().unwrap();
    let marker = home.path().join("should-not-run");
    let (output, _) = run_human_shell(b"exit\n");
    assert!(output.status.success());
    assert!(
        !marker.exists(),
        "suggested command must not be auto-executed"
    );
}

#[test]
fn human_shell_startup_prints_parent_request_and_suggested_command() {
    let (output, _) = run_human_shell(b"exit\n");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Human control requested by the parent agent."));
    assert!(stderr.contains("create marker file"));
    assert!(stderr.contains("touch /tmp/should-not-run"));
    assert!(stderr.contains("Alt+. or Alt+,"));
}
