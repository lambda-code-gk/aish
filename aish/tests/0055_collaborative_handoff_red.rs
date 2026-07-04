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
        .env(
            "AI_SUGGESTION_CACHE",
            home.path().join("shared-suggestions.json"),
        )
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
fn human_shell_preserves_shared_suggestion_cache_through_pty_and_rcfile() {
    let (output, _) = run_human_shell(b"printf 'CACHE=%s\n' \"$AI_SUGGESTION_CACHE\"\nexit\n");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("shared-suggestions.json"), "{stdout}");
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
fn ctrl_c_in_human_shell_does_not_terminate_parent() {
    if !std::path::Path::new("/bin/bash").is_file() {
        return;
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"sleep 30\n\x03\nexit\n")
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("human shell hung after ctrl-c")
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: aish::human_shell::HumanShellResult =
        serde_json::from_str(&std::fs::read_to_string(result_file).unwrap()).unwrap();
    assert!(result.normal_return);
}

#[test]
fn human_shell_job_control_fg_bg() {
    if !std::path::Path::new("/bin/bash").is_file() {
        return;
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"sleep 30\n\x1a\nbg\nfg\nexit\n")
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(12))
        .expect("human shell hung during job control")
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn human_shell_prompt_shows_collaborative_status() {
    let tmp = tempfile::tempdir().unwrap();
    let handoff_dir = tmp.path().join("ho-test");
    std::fs::create_dir_all(&handoff_dir).unwrap();
    std::fs::write(
        handoff_dir.join("handoff.json"),
        r#"{"state":"HUMAN_ACTIVE"}"#,
    )
    .unwrap();
    unsafe {
        std::env::set_var("AISH_CONTROL_MODE", "human-shell");
        std::env::set_var("AISH_HANDOFF_ID", "ho-test");
        std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        std::env::remove_var("AISH_COLLABORATIVE_PROMPT_TEMPLATE");
    }
    let prefix = aish::collaborative_prompt::render_collaborative_prompt_prefix();
    unsafe {
        std::env::remove_var("AISH_CONTROL_MODE");
        std::env::remove_var("AISH_HANDOFF_ID");
        std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
    }
    assert!(prefix.contains("collab"), "prefix={prefix}");
    assert!(prefix.contains("human"), "prefix={prefix}");
}

#[test]
fn human_shell_prompt_shows_waiting_for_side_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let handoff_dir = tmp.path().join("ho-test");
    std::fs::create_dir_all(&handoff_dir).unwrap();
    std::fs::write(
        handoff_dir.join("handoff.json"),
        r#"{"state":"SIDE_AGENT_WAITING_FOR_HUMAN"}"#,
    )
    .unwrap();
    unsafe {
        std::env::set_var("AISH_CONTROL_MODE", "human-shell");
        std::env::set_var("AISH_HANDOFF_ID", "ho-test");
        std::env::set_var("AISH_HANDOFF_STORE_ROOT", tmp.path());
        std::env::remove_var("AISH_COLLABORATIVE_PROMPT_TEMPLATE");
    }
    let prefix = aish::collaborative_prompt::render_collaborative_prompt_prefix();
    unsafe {
        std::env::remove_var("AISH_CONTROL_MODE");
        std::env::remove_var("AISH_HANDOFF_ID");
        std::env::remove_var("AISH_HANDOFF_STORE_ROOT");
    }
    assert!(prefix.contains("waiting"), "prefix={prefix}");
    assert!(prefix.contains("ai"), "prefix={prefix}");
}

#[test]
fn heartbeat_maintains_lease_during_long_command() {
    use std::io::Write;
    if !std::path::Path::new("/bin/bash").is_file() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let store_root = tmp.path().join("handoffs");
    let handoff_dir = store_root.join("handoff-test");
    std::fs::create_dir_all(&handoff_dir).unwrap();
    let lease_path = handoff_dir.join("lease.json");
    let future = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 120_000;
    let lease = serde_json::json!({
        "handoff_id": "handoff-test",
        "owner_client_id": "owner",
        "owner_process_id": 1,
        "owner_tty": null,
        "owner_host": "host",
        "owner_uid": 1000,
        "lease_acquired_at_ms": 1,
        "lease_expires_at_ms": future,
        "last_heartbeat_at_ms": 1
    });
    std::fs::write(&lease_path, serde_json::to_vec(&lease).unwrap()).unwrap();
    let result_file = tmp.path().join("result.json");
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_aish"))
        .args(["human-shell", "--result-file"])
        .arg(&result_file)
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .env("AISH_CONTROL_MODE", "human-shell")
        .env("AISH_HANDOFF_ID", "handoff-test")
        .env("AISH_HANDOFF_TOKEN", "opaque-test-token")
        .env("AISH_HANDOFF_CONTEXT_VERSION", "1")
        .env("AISH_HANDOFF_STORE_ROOT", &store_root)
        .env("AISH_HANDOFF_HEARTBEAT_INTERVAL_MS", "200")
        .env("AISH_HANDOFF_LEASE_TIMEOUT_MS", "120000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(250));
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"sleep 1\nexit\n")
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = rx
        .recv_timeout(Duration::from_secs(8))
        .expect("human shell hung during heartbeat test")
        .unwrap();
    assert!(output.status.success());
    let updated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&lease_path).unwrap()).unwrap();
    assert!(updated["last_heartbeat_at_ms"].as_u64().unwrap() > 1);
    assert!(updated["lease_expires_at_ms"].as_u64().unwrap() > future);
}
