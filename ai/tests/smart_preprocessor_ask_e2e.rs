#![cfg(unix)]
//! Smart Preprocessor の `ai ask` 導通（preprocessor → route_turn → agent_turn）。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, RouteTurnStatus,
};

struct MockSocketServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
    route_turn_count: Arc<Mutex<usize>>,
}

impl MockSocketServer {
    fn with_expected_route_turns(expected: usize) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let route_turn_count = Arc::new(Mutex::new(0usize));
        let count_clone = Arc::clone(&route_turn_count);
        let handle = thread::spawn(move || {
            let mut handled = 0usize;
            let max = if expected == 0 { 1 } else { expected + 1 };
            while handled < max {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                line.clear();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                if line.trim().is_empty() {
                    continue;
                }
                let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
                let response = match req {
                    ClientRequest::RouteTurn { conversation, .. } => {
                        *count_clone.lock().expect("lock") += 1;
                        if let Some(ref summary) = conversation.recent_summary {
                            assert!(
                                !summary.is_empty(),
                                "assist should pass bounded recent_summary"
                            );
                        }
                        ClientResponse::RouteTurnResult {
                            id: "route-1".into(),
                            status: RouteTurnStatus::Ok,
                            plan: serde_json::from_str(
                                r#"{
                                  "conversation_id": "conv-pre",
                                  "new_conversation": true,
                                  "route_kind": "chat",
                                  "require_shell_approval": false,
                                  "log_tail_escalation": false,
                                  "route_reason": "preprocessor e2e"
                                }"#,
                            )
                            .expect("plan json"),
                        }
                    }
                    ClientRequest::AgentTurn { messages, .. } => {
                        assert!(
                            messages.iter().any(|m| {
                                m.role == "user"
                                    && (m.content.contains("ping")
                                        || m.content.contains("エラー")
                                        || m.content.contains("error"))
                            }),
                            "user message must reach agent_turn"
                        );
                        ClientResponse::AgentTurnResult {
                            id: "turn-1".into(),
                            status: AgentTurnStatus::Ok,
                            assistant_message: ProtocolMessageOut {
                                role: "assistant".into(),
                                content: "preprocessor ok".into(),
                            },
                            tool_calls: vec![],
                        }
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                handled += 1;
                let payload = serde_json::to_string(&response).expect("serialize");
                writeln!(writer, "{payload}").expect("write");
                writer.flush().expect("flush");
            }
        });
        Self {
            handle: Some(handle),
            _dir: dir,
            socket_path,
            route_turn_count,
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

fn write_ai_config(
    socket_path: &std::path::Path,
    dir: &tempfile::TempDir,
    preprocessor_toml: &str,
) -> std::path::PathBuf {
    let config_path = dir.path().join("ai.toml");
    let observation_path = dir.path().join("observation.jsonl");
    fs::write(
        &config_path,
        format!(
            r#"
socket_path = "{}"
[smart_preprocessor]
enabled = true
observation_path = "{}"
{}
"#,
            socket_path.display(),
            observation_path.display(),
            preprocessor_toml
        ),
    )
    .expect("write config");
    config_path
}

fn script_available() -> bool {
    Command::new("script")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_tty_ask(cfg: &std::path::Path, home: &std::path::Path, query: &str) -> String {
    let ai_bin = env!("CARGO_BIN_EXE_ai");
    let inner = format!("{ai_bin} --no-start --no-progress '{query}'");
    let transcript = home.join("typescript");
    let status = Command::new("script")
        .arg("-q")
        .arg("-c")
        .arg(&inner)
        .arg(&transcript)
        .env("AI_CONFIG", cfg)
        .env("HOME", home)
        .status()
        .expect("run script+ai");
    assert!(status.success(), "ai exited with {status}");
    fs::read_to_string(&transcript).unwrap_or_default()
}

#[test]
fn shadow_mode_calls_route_turn_and_writes_observation() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        r#"
mode = "shadow"
model_path = "model.json"
"#,
    );
    let captured = run_tty_ask(&cfg, home.path(), "ping preprocessor shadow");
    assert!(
        captured.contains("preprocessor ok"),
        "transcript: {captured}"
    );
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
    let obs = fs::read_to_string(home.path().join("observation.jsonl")).unwrap_or_default();
    assert!(obs.contains("shadow") || obs.contains("Shadow"));
    assert!(!obs.contains("ghp_"));
}

#[test]
fn assist_mode_passes_bounded_summary_to_route_turn() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        r#"
mode = "assist"
model_path = "model.json"
"#,
    );
    unsafe {
        std::env::set_var("AISH_SESSION_DIR", home.path());
    }
    fs::write(
        home.path().join("session.jsonl"),
        r#"{"event":"error","message":"test failed: foo"}"#,
    )
    .expect("session log");
    let captured = run_tty_ask(&cfg, home.path(), "さっきのエラーを直して");
    assert!(
        captured.contains("preprocessor ok"),
        "transcript: {captured}"
    );
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
    unsafe {
        std::env::remove_var("AISH_SESSION_DIR");
    }
}
