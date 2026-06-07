#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread::{self, JoinHandle};

use aibe_protocol::{AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut};
use serde_json::Value;

struct MockSocketServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
}

impl MockSocketServer {
    fn ping() -> Self {
        Self::spawn(|req| match req {
            ClientRequest::Ping { .. } => ClientResponse::Pong {
                id: "pong-1".to_string(),
            },
            other => panic!("unexpected request: {other:?}"),
        })
    }

    fn ask(expected_message: &'static str, assistant: &'static str) -> Self {
        Self::spawn(move |req| match req {
            ClientRequest::AgentTurn { messages, .. } => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].content, expected_message);
                ClientResponse::AgentTurnResult {
                    id: "turn-1".to_string(),
                    status: AgentTurnStatus::Ok,
                    assistant_message: ProtocolMessageOut {
                        role: "assistant".to_string(),
                        content: assistant.to_string(),
                    },
                    tool_calls: vec![],
                }
            }
            other => panic!("unexpected request: {other:?}"),
        })
    }

    fn spawn(responder: impl Fn(ClientRequest) -> ClientResponse + Send + 'static) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read request");
            let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
            let response = responder(req);
            let payload = serde_json::to_string(&response).expect("serialize response");
            writeln!(writer, "{payload}").expect("write response");
        });

        Self {
            handle: Some(handle),
            _dir: dir,
            socket_path,
        }
    }
}

impl Drop for MockSocketServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_ai_config(socket_path: &std::path::Path, dir: &tempfile::TempDir) -> std::path::PathBuf {
    let config_path = dir.path().join("ai.toml");
    fs::write(
        &config_path,
        format!(
            r#"
socket_path = "{}"
[ask]
default_profile = "fast"
filter = "cat"
"#,
            socket_path.display()
        ),
    )
    .expect("write config");
    config_path
}

fn setup_session_dir(root: &tempfile::TempDir) -> std::path::PathBuf {
    let session = root.path().join("002f15d02b54");
    fs::create_dir(&session).expect("mkdir");
    fs::write(session.join("log.jsonl"), "{}\n").expect("log");
    symlink("log.jsonl", session.join("current_log")).expect("symlink");
    session
}

#[test]
fn default_ask_supports_json_format_and_quiet() {
    let server = MockSocketServer::ask("hello", "assistant says hi");
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["--quiet", "--format", "json", "--no-start", "hello"])
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["response_type"], "agent_turn_result");
    assert_eq!(json["assistant_message"]["content"], "assistant says hi");
}

#[test]
fn status_reports_config_session_and_socket() {
    let server = MockSocketServer::ping();
    let home = tempfile::tempdir().expect("home");
    let session_root = tempfile::tempdir().expect("session_root");
    let session_dir = setup_session_dir(&session_root);
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["status", "--quiet", "--format", "json"])
        .output()
        .expect("run ai status");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["command"], "status");
    assert_eq!(json["socket_alive"], true);
    assert_eq!(
        json["config_socket_path"],
        server.socket_path.display().to_string()
    );
    assert_eq!(json["implicit_session_id"], "002f15d02b54");
    assert_eq!(
        json["shell_log_path"],
        session_dir.join("log.jsonl").display().to_string()
    );
}

#[test]
fn doctor_alias_uses_doctor_command_name() {
    let server = MockSocketServer::ping();
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["doctor", "--quiet", "--format", "json"])
        .output()
        .expect("run ai doctor");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["command"], "doctor");
}

#[test]
fn ping_reports_failure_without_socket() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(&cfg_path, "socket_path = \"/tmp/does-not-exist.sock\"\n").expect("write config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["ping", "--quiet", "--format", "tsv"])
        .output()
        .expect("run ai ping");

    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(stdout.contains("socket.alive\tfalse"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ai:"));
}

#[test]
fn dry_run_masks_message_and_skips_aibe() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(&cfg_path, "socket_path = \"/tmp/does-not-exist.sock\"\n").expect("write config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["--quiet", "--dry-run", "--format", "json", "hello"])
        .output()
        .expect("run ai dry-run");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["message_masked"], "<masked>");
    assert_eq!(json["message_source"], "argv");
}

#[test]
fn non_tty_ask_skips_route_turn_and_injects_ai_session_id() {
    let server = MockSocketServer::spawn(|req| match req {
        ClientRequest::AgentTurn {
            messages, context, ..
        } => {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].content, "hello");
            let session_id = context.ai_session_id.expect("ai_session_id");
            assert!(!session_id.is_empty());
            assert!(context.conversation_id.is_none());
            ClientResponse::AgentTurnResult {
                id: "turn-1".to_string(),
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".to_string(),
                    content: "assistant says hi".to_string(),
                },
                tool_calls: vec![],
            }
        }
        other => panic!("unexpected request: {other:?}"),
    });
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["--quiet", "--no-start", "hello"])
        .output()
        .expect("run ai");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    assert!(stdout.contains("assistant says hi"));
}
