// 0055 Collaborative Human Handoff acceptance tests.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn run_human_shell(input: &[u8]) -> (std::process::Output, aish::human_shell::HumanShellResult) {
    if !std::path::Path::new("/bin/bash").is_file() {
        panic!("Phase 2 human-shell tests require /bin/bash");
    }
    let home = tempfile::tempdir().unwrap();
    let result_file = home.path().join("result.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_ID", "ho-test")
        .env("AISH_HANDOFF_TOKEN", "opaque-test-token")
        .env("AISH_HANDOFF_CONTEXT_VERSION", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    child.stdin.take().unwrap().write_all(input).unwrap();
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
    assert!(aish::human_shell::handoff_environment_is_complete([
        ("AISH_CONTROL_MODE", "human-shell"),
        ("AISH_HANDOFF_ID", "ho-1"),
        ("AISH_HANDOFF_TOKEN", "secret"),
        ("AISH_HANDOFF_CONTEXT_VERSION", "1"),
    ]));
    let (output, _) = run_human_shell(b"printf '%s|%s|%s|%s\\n' \"$AISH_CONTROL_MODE\" \"$AISH_HANDOFF_ID\" \"$AISH_HANDOFF_TOKEN\" \"$AISH_HANDOFF_CONTEXT_VERSION\"\nexit\n");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("human-shell|ho-test|opaque-test-token|1")
    );
}

#[test]
fn human_shell_ctrl_d_returns_control_to_parent() {
    let (output, actual) = run_human_shell(b"\x04");
    assert!(output.status.success());
    assert!(actual.normal_return);
    let marker = aish::adapters::outbound::HumanReturnMarker {
        exit_code: Some(0),
        final_cwd: "/tmp/work".into(),
    };
    let result = aish::human_shell::human_shell_result_from_marker(marker, 0);
    assert!(result.normal_return);
    assert_eq!(result.final_cwd.to_str(), Some("/tmp/work"));
}

#[test]
fn human_shell_exit_returns_control_regardless_of_code() {
    for (input, code) in [(b"exit\n".as_slice(), 0), (b"exit 1\n".as_slice(), 1)] {
        let (output, actual) = run_human_shell(input);
        assert!(output.status.success());
        assert_eq!(actual.exit_code, Some(code));
    }
    for code in [0, 1, 42] {
        let marker = aish::adapters::outbound::HumanReturnMarker {
            exit_code: Some(code),
            final_cwd: "/tmp".into(),
        };
        let result = aish::human_shell::human_shell_result_from_marker(marker, code);
        assert!(result.normal_return);
        assert_eq!(result.exit_code, Some(code));
    }
}

#[test]
#[ignore = "0055 phase5: ctrl_c_in_human_shell_does_not_terminate_parent"]
fn ctrl_c_in_human_shell_does_not_terminate_parent() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: human_shell_job_control_fg_bg"]
fn human_shell_job_control_fg_bg() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: human_shell_prompt_shows_collaborative_status"]
fn human_shell_prompt_shows_collaborative_status() {
    panic!("0055 phase5 not implemented");
}

#[test]
#[ignore = "0055 phase5: human_shell_prompt_shows_waiting_for_side_agent"]
fn human_shell_prompt_shows_waiting_for_side_agent() {
    panic!("0055 phase5 not implemented");
}
