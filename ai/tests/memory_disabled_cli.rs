#![cfg(feature = "memory")]
#![cfg(unix)]

use std::fs;
use std::process::Command;

#[test]
fn goal_set_fails_when_memory_disabled_in_config() {
    let home = tempfile::tempdir().expect("tempdir");
    let config_path = home.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[memory]
enabled = false
"#,
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", &config_path)
        .env("AI_SESSION_ID", "mem-disabled-test")
        .args(["goal", "set", "--no-start", "ship memory"])
        .output()
        .expect("run ai goal set");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "goal set should fail when memory disabled, stderr={stderr}"
    );
    assert!(
        stderr.contains("contextual memory is disabled"),
        "stderr={stderr}"
    );
}

#[test]
fn ask_omits_memory_space_id_when_memory_disabled() {
    let home = tempfile::tempdir().expect("tempdir");
    let config_path = home.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[memory]
enabled = false
"#,
    )
    .expect("write config");

    let dir = tempfile::tempdir().expect("socket dir");
    let socket_path = dir.path().join("mock.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket_path).expect("bind");

    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        use std::io::{Read, Write};
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).expect("read");
        let line = std::str::from_utf8(&buf[..n]).expect("utf8");
        assert!(
            !line.contains("memory_space_id"),
            "agent_turn should not send memory_space_id when memory disabled: {line}"
        );
        let resp = r#"{"type":"agent_turn_result","id":"turn-1","status":"ok","assistant_message":{"role":"assistant","content":"ok"},"tool_calls":[]}"#;
        writeln!(stream, "{resp}").expect("write");
    });

    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", &config_path)
        .env("AIBE_SOCKET_PATH", &socket_path)
        .env("AI_SESSION_ID", "mem-disabled-ask")
        .args(["ask", "--no-start", "hello"])
        .output()
        .expect("run ai ask");

    server.join().expect("server join");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn context_current_fails_when_memory_disabled_in_config() {
    let home = tempfile::tempdir().expect("tempdir");
    let config_path = home.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[memory]
enabled = false
"#,
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", &config_path)
        .env("AI_SESSION_ID", "mem-disabled-context")
        .args(["context", "current"])
        .output()
        .expect("run ai context current");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "context current should fail when memory disabled, stderr={stderr}"
    );
    assert!(
        stderr.contains("contextual memory is disabled"),
        "stderr={stderr}"
    );
}

#[test]
fn memory_disabled_via_env_overrides_config_enabled() {
    let home = tempfile::tempdir().expect("tempdir");
    let config_path = home.path().join("config.toml");
    fs::write(
        &config_path,
        r#"
[memory]
enabled = true
"#,
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", &config_path)
        .env("AI_MEMORY_ENABLED", "0")
        .env("AI_SESSION_ID", "mem-env-disabled")
        .args(["goal", "set", "--no-start", "ship memory"])
        .output()
        .expect("run ai goal set");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "goal set should fail when AI_MEMORY_ENABLED=0, stderr={stderr}"
    );
    assert!(
        stderr.contains("contextual memory is disabled"),
        "stderr={stderr}"
    );
}
