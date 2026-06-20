#![cfg(unix)]
//! Smart Preprocessor の `ai ask` 導通（preprocessor → route_turn → agent_turn）。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut, RouteTurnConversation,
    RouteTurnStatus,
};

struct MockSocketServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
    route_turn_count: Arc<Mutex<usize>>,
    agent_turn_count: Arc<Mutex<usize>>,
    last_conversation: Arc<Mutex<Option<RouteTurnConversation>>>,
}

impl MockSocketServer {
    fn with_expected_route_turns(expected: usize) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let route_turn_count = Arc::new(Mutex::new(0usize));
        let count_clone = Arc::clone(&route_turn_count);
        let agent_turn_count = Arc::new(Mutex::new(0usize));
        let agent_count_clone = Arc::clone(&agent_turn_count);
        let last_conversation = Arc::new(Mutex::new(None));
        let conversation_clone = Arc::clone(&last_conversation);
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
                        *conversation_clone.lock().expect("lock") = Some(conversation.clone());
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
                        *agent_count_clone.lock().expect("lock") += 1;
                        assert!(
                            messages.iter().any(|m| {
                                m.role == "user"
                                    && (m.content.contains("ping")
                                        || m.content.contains("hello")
                                        || m.content.contains("エラー")
                                        || m.content.contains("error")
                                        || m.content.contains("git")
                                        || m.content.contains("設計")
                                        || m.content.contains("memory")
                                        || m.content.contains("recipe")
                                        || m.content.contains("sudo"))
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
            agent_turn_count,
            last_conversation,
        }
    }

    fn agent_turn_requests(&self) -> usize {
        *self.agent_turn_count.lock().expect("lock")
    }

    fn route_turn_requests(&self) -> usize {
        *self.route_turn_count.lock().expect("lock")
    }

    fn last_route_turn_conversation(&self) -> Option<RouteTurnConversation> {
        self.last_conversation.lock().expect("lock").clone()
    }
}

impl Drop for MockSocketServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn bundled_model_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/smart_preprocessor_model.json")
}

fn install_model(home: &std::path::Path) -> std::path::PathBuf {
    let model_path = home.join("model.json");
    fs::copy(bundled_model_path(), &model_path).expect("copy model");
    model_path
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

fn run_tty_ask(
    cfg: &std::path::Path,
    home: &std::path::Path,
    query: &str,
    extra_env: &[(&str, &str)],
) -> String {
    let ai_bin = env!("CARGO_BIN_EXE_ai");
    let inner = format!("{ai_bin} --no-start --no-progress '{query}'");
    let transcript = home.join("typescript");
    let mut cmd = Command::new("script");
    cmd.arg("-q")
        .arg("-c")
        .arg(&inner)
        .arg(&transcript)
        .env("AI_CONFIG", cfg)
        .env("HOME", home)
        .env_remove("AISH_SESSION_DIR");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let status = cmd.status().expect("run script+ai");
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
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "shadow"
model_path = "{}"
max_observation_bytes = 2048
"#,
            model_path.display()
        ),
    );
    let captured = run_tty_ask(&cfg, home.path(), "ping preprocessor shadow", &[]);
    assert!(
        captured.contains("preprocessor ok"),
        "transcript: {captured}"
    );
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
    let obs = fs::read_to_string(home.path().join("observation.jsonl")).unwrap_or_default();
    assert!(obs.contains("shadow") || obs.contains("\"mode\":\"shadow\""));
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
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "assist"
model_path = "{}"
max_observation_bytes = 2048
"#,
            model_path.display()
        ),
    );
    fs::write(
        home.path().join("session.jsonl"),
        r#"{"event":"error","message":"test failed: foo"}"#,
    )
    .expect("session log");
    let captured = run_tty_ask(
        &cfg,
        home.path(),
        "さっきのエラーを直して",
        &[("AISH_SESSION_DIR", home.path().to_str().expect("utf8"))],
    );
    assert!(
        captured.contains("preprocessor ok"),
        "transcript: {captured}"
    );
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
}

#[test]
fn gate_mode_skips_route_turn_for_simple_chat() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(0);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 2048
"#,
            model_path.display()
        ),
    );
    let captured = run_tty_ask(&cfg, home.path(), "hello", &[]);
    assert!(
        captured.contains("preprocessor ok"),
        "transcript: {captured}"
    );
    assert_eq!(
        *server.route_turn_count.lock().expect("lock"),
        0,
        "gate short-circuit must skip route_turn"
    );
    let obs = fs::read_to_string(home.path().join("observation.jsonl")).unwrap_or_default();
    assert!(obs.contains("gate_short_circuit") || obs.contains("\"mode\":\"gate\""));
    assert!(obs.contains("\"route_turn_hints_injected\":false"));
}

fn assist_preprocessor_toml(model_path: &std::path::Path) -> String {
    format!(
        r#"
mode = "assist"
model_path = "{}"
max_observation_bytes = 2048
assist_threshold = 0.50
"#,
        model_path.display()
    )
}

#[test]
fn memory_lookup_keeps_route_turn_and_injects_hints() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &assist_preprocessor_toml(&model_path),
    );
    let _ = run_tty_ask(&cfg, home.path(), "前に決めた設計方針を教えて", &[]);
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
    let conversation = server
        .last_route_turn_conversation()
        .expect("route_turn conversation");
    let hints = conversation.preprocessor_hints.expect("preprocessor hints");
    assert!(hints.tool_hints.iter().any(|h| h == "memory_search"));
}

#[test]
fn memory_recipe_hint_keeps_route_turn_and_injects_hints() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &assist_preprocessor_toml(&model_path),
    );
    let _ = run_tty_ask(&cfg, home.path(), "memory recipe を実行して", &[]);
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 1);
    let conversation = server
        .last_route_turn_conversation()
        .expect("route_turn conversation");
    assert!(conversation.preprocessor_hints.is_some());
}

#[test]
fn git_diff_consultation_injects_context_needs() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &assist_preprocessor_toml(&model_path),
    );
    let _ = run_tty_ask(&cfg, home.path(), "git diff を見て", &[]);
    let hints = server
        .last_route_turn_conversation()
        .and_then(|c| c.preprocessor_hints)
        .expect("preprocessor hints");
    assert!(hints.context_needs.iter().any(|n| n == "vcs_status"));
    assert!(hints.context_needs.iter().any(|n| n == "vcs_diff"));
}

#[test]
fn session_error_summary_injects_failure_kind_into_route_turn() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &assist_preprocessor_toml(&model_path),
    );
    fs::write(
        home.path().join("session.jsonl"),
        r#"{"event":"error","message":"permission denied"}"#,
    )
    .expect("session log");
    let _ = run_tty_ask(
        &cfg,
        home.path(),
        "このエラーを直して",
        &[("AISH_SESSION_DIR", home.path().to_str().expect("utf8"))],
    );
    let hints = server
        .last_route_turn_conversation()
        .and_then(|c| c.preprocessor_hints)
        .expect("preprocessor hints");
    assert_eq!(hints.failure_kind.as_deref(), Some("permission"));
}

#[test]
fn gate_short_circuit_skips_route_turn_request() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(0);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 2048
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(&cfg, home.path(), "hello", &[]);
    assert_eq!(*server.route_turn_count.lock().expect("lock"), 0);
    assert!(server.last_route_turn_conversation().is_none());
}

#[test]
fn unsafe_input_skips_short_circuit_but_injects_hints_when_needed() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 2048
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(
        &cfg,
        home.path(),
        "sudo rm -rf /tmp/foo の git diff を見て",
        &[],
    );
    assert_eq!(
        *server.route_turn_count.lock().expect("lock"),
        1,
        "unsafe input must not short-circuit"
    );
    let hints = server
        .last_route_turn_conversation()
        .and_then(|c| c.preprocessor_hints)
        .expect("hints on unsafe git consult");
    assert!(hints.context_needs.iter().any(|n| n == "vcs_diff"));
}

#[test]
fn local_route_skips_route_turn_for_high_confidence_safe_input() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(0);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
[ask]
tools = "@read-only"
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(&cfg, home.path(), "git diff を見て", &[]);
    assert_eq!(
        *server.route_turn_count.lock().expect("lock"),
        0,
        "high confidence safe tool-backed inspection should use local route when vcs tools are allowed"
    );
    assert!(server.last_route_turn_conversation().is_none());
}

#[test]
fn local_route_falls_back_to_route_turn_for_medium_or_unsafe_input() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(
        &cfg,
        home.path(),
        "sudo rm -rf /tmp/foo の git diff を見て",
        &[],
    );
    assert_eq!(
        *server.route_turn_count.lock().expect("lock"),
        1,
        "unsafe input must fall back to route_turn"
    );
}

#[test]
fn local_route_still_calls_agent_turn() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(0);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
[ask]
tools = "@read-only"
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(&cfg, home.path(), "git diff を見て", &[]);
    assert_eq!(server.route_turn_requests(), 0);
    assert_eq!(server.agent_turn_requests(), 1);
}

#[test]
fn local_route_observation_includes_agent_turn_latency() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(0);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let observation_path = home.path().join("observation.jsonl");
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
[ask]
tools = "@read-only"
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(&cfg, home.path(), "git diff を見て", &[]);
    let obs = fs::read_to_string(&observation_path).unwrap_or_default();
    let line = obs.lines().last().expect("observation line");
    let value: serde_json::Value = serde_json::from_str(line).expect("json");
    assert_eq!(value["route_turn_used"], false);
    assert_eq!(value["agent_turn_used"], true);
    assert!(value.get("agent_turn_latency_ms").is_some());
    assert!(value.get("total_turn_latency_ms").is_some());
}

#[test]
fn route_turn_fallback_observation_marks_route_turn_used() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let observation_path = home.path().join("observation.jsonl");
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(
        &cfg,
        home.path(),
        "sudo rm -rf /tmp/foo の git diff を見て",
        &[],
    );
    assert_eq!(server.route_turn_requests(), 1);
    let obs = fs::read_to_string(&observation_path).unwrap_or_default();
    let line = obs.lines().last().expect("observation line");
    let value: serde_json::Value = serde_json::from_str(line).expect("json");
    assert_eq!(value["route_turn_used"], true);
    assert_eq!(value["agent_turn_used"], true);
    assert_eq!(value["decision_path"], "local_route_fallback");
    assert_eq!(value["fallback_reason"].as_str(), Some("unsafe"));
}

#[test]
fn local_route_missing_tools_fallback_reason_in_observation() {
    if !script_available() {
        eprintln!("skip: script(1) not available for pseudo-tty");
        return;
    }
    let server = MockSocketServer::with_expected_route_turns(1);
    let home = tempfile::tempdir().expect("home");
    let model_path = install_model(home.path());
    let observation_path = home.path().join("observation.jsonl");
    let cfg = write_ai_config(
        &server.socket_path,
        &home,
        &format!(
            r#"
mode = "gate"
model_path = "{}"
max_observation_bytes = 4096
"#,
            model_path.display()
        ),
    );
    let _ = run_tty_ask(&cfg, home.path(), "git diff を見て", &[]);
    assert_eq!(server.route_turn_requests(), 1);
    let obs = fs::read_to_string(&observation_path).unwrap_or_default();
    let line = obs.lines().last().expect("observation line");
    let value: serde_json::Value = serde_json::from_str(line).expect("json");
    assert_eq!(value["route_turn_used"], true);
    assert_eq!(
        value["fallback_reason"].as_str(),
        Some("missing_required_local_tool")
    );
}
