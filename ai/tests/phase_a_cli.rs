#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, MemoryApplyStatus, MemoryOperationDto,
    MemoryQueryStatus, MemoryScopeDto, ProtocolMessageOut,
};
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
        Self::spawn_multi(move |req| vec![responder(req)])
    }

    fn ask_with_streaming(
        expected_message: &'static str,
        stream_chunks: &'static [&'static str],
        final_content: &'static str,
    ) -> Self {
        Self::spawn_multi(move |req| match req {
            ClientRequest::AgentTurn { id, messages, .. } => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].content, expected_message);
                let mut responses = stream_chunks
                    .iter()
                    .map(|chunk| ClientResponse::AssistantStreaming {
                        id: id.clone(),
                        delta: (*chunk).to_string(),
                    })
                    .collect::<Vec<_>>();
                responses.push(ClientResponse::AgentTurnResult {
                    id,
                    status: AgentTurnStatus::Ok,
                    assistant_message: ProtocolMessageOut {
                        role: "assistant".to_string(),
                        content: final_content.to_string(),
                    },
                    tool_calls: vec![],
                });
                responses
            }
            other => panic!("unexpected request: {other:?}"),
        })
    }

    fn spawn_multi(
        responder: impl Fn(ClientRequest) -> Vec<ClientResponse> + Send + 'static,
    ) -> Self {
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
            for response in responder(req) {
                let payload = serde_json::to_string(&response).expect("serialize response");
                writeln!(writer, "{payload}").expect("write response");
                writer.flush().expect("flush response");
            }
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
fn structured_format_ignores_stream_chunks_on_stdout() {
    let home = tempfile::tempdir().expect("home");

    let server_json =
        MockSocketServer::ask_with_streaming("hello", &["partial ", "stream "], "final answer");
    let cfg_json = write_ai_config(&server_json.socket_path, &home);
    let out_json = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_json)
        .env("HOME", home.path())
        .args(["--format", "json", "--no-start", "--no-progress", "hello"])
        .output()
        .expect("run ai json");
    assert!(
        out_json.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out_json.stderr)
    );
    let json: Value = serde_json::from_slice(&out_json.stdout).expect("json");
    assert_eq!(json["response_type"], "agent_turn_result");
    assert_eq!(json["assistant_message"]["content"], "final answer");

    let server_tsv =
        MockSocketServer::ask_with_streaming("hello", &["partial ", "stream "], "final answer");
    let cfg_tsv = write_ai_config(&server_tsv.socket_path, &home);
    let out_tsv = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_tsv)
        .env("HOME", home.path())
        .args(["--format", "tsv", "--no-start", "--no-progress", "hello"])
        .output()
        .expect("run ai tsv");
    assert!(
        out_tsv.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out_tsv.stderr)
    );
    let tsv = String::from_utf8_lossy(&out_tsv.stdout);
    assert!(tsv.contains("response_type\tagent_turn_result"));
    assert!(tsv.contains("assistant_message.content\tfinal answer"));
    assert!(!tsv.contains("partial "));

    let server_env =
        MockSocketServer::ask_with_streaming("hello", &["partial ", "stream "], "final answer");
    let cfg_env = write_ai_config(&server_env.socket_path, &home);
    let out_env = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_env)
        .env("HOME", home.path())
        .args(["--format", "env", "--no-start", "--no-progress", "hello"])
        .output()
        .expect("run ai env");
    assert!(
        out_env.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out_env.stderr)
    );
    let env_out = String::from_utf8_lossy(&out_env.stdout);
    assert!(env_out.contains("AI_RESPONSE_TYPE='agent_turn_result'"));
    assert!(env_out.contains("AI_ASSISTANT_MESSAGE_CONTENT='final answer'"));
    assert!(!env_out.contains("partial "));
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

#[test]
fn dry_run_reports_console_hint_resolution() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(&cfg_path, "socket_path = \"/tmp/does-not-exist.sock\"\n").expect("write config");

    for disable_flag in ["--no-console-hint", "-N"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ai"))
            .env("AI_CONFIG", &cfg_path)
            .env("HOME", home.path())
            .args([disable_flag, "--dry-run", "--format", "json", "hello"])
            .output()
            .expect("run ai dry-run");

        assert!(out.status.success(), "flag {disable_flag}");
        let json: Value = serde_json::from_slice(&out.stdout).expect("json");
        assert_eq!(json["console_hint"]["requested"], false);
        assert_eq!(json["console_hint"]["source"], "cli");
        assert_eq!(json["console_hint"]["effective"], false);
    }
}

#[test]
fn console_hint_short_option_maps_to_enable() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(&cfg_path, "socket_path = \"/tmp/does-not-exist.sock\"\n").expect("write config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["-H", "--dry-run", "--format", "json", "hello"])
        .output()
        .expect("run ai dry-run");

    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["console_hint"]["requested"], true);
    assert_eq!(json["console_hint"]["source"], "cli");
    assert_eq!(json["console_hint"]["effective"], false);
    // 非 TTY のテスト環境では tty 抑止が format より優先される。
    assert_eq!(json["console_hint"]["suppressed_by"], "tty");
}

#[test]
fn config_console_hints_false_applies_without_cli_flag() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(
        &cfg_path,
        r#"
socket_path = "/tmp/does-not-exist.sock"
[ask]
console_hints = false
"#,
    )
    .expect("write config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["--dry-run", "--format", "json", "hello"])
        .output()
        .expect("run ai dry-run");

    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["console_hint"]["requested"], false);
    assert_eq!(json["console_hint"]["source"], "config");
}

#[test]
fn no_console_hint_skips_system_instruction_on_agent_turn() {
    let server = MockSocketServer::spawn(|req| match req {
        ClientRequest::AgentTurn { context, .. } => {
            assert!(context.system_instruction.is_none());
            ClientResponse::AgentTurnResult {
                id: "turn-1".to_string(),
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".to_string(),
                    content: "ok".to_string(),
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
        .args(["--quiet", "--no-start", "--no-console-hint", "hello"])
        .output()
        .expect("run ai");

    assert!(out.status.success());
}

#[test]
fn goal_set_sends_memory_apply() {
    let server = MockSocketServer::spawn(|req| match req {
        ClientRequest::MemoryApply(body) => {
            assert!(!body.session_id.is_empty());
            assert!(!body.context.cwd.is_empty());
            let operation = body.operation;
            assert!(matches!(
                operation,
                MemoryOperationDto::Add {
                    kind,
                    scope: MemoryScopeDto::Project,
                    make_active: true,
                    ..
                } if kind == "goal"
            ));
            ClientResponse::MemoryApplyResult {
                id: "m1".to_string(),
                status: MemoryApplyStatus::Ok,
                entries: vec![],
            }
        }
        other => panic!("unexpected request: {other:?}"),
    });
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AI_SESSION_ID", "phase-a-memory")
        .args(["goal", "set", "--no-start", "ship memory"])
        .output()
        .expect("run ai goal set");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("goal set: ship memory"));
}

#[test]
fn mem_show_requests_prompt_block() {
    let server = MockSocketServer::spawn(|req| match req {
        ClientRequest::MemoryQuery(body) => {
            assert!(body.query.include_prompt_block);
            assert!(body.query.user_query.is_none());
            ClientResponse::MemoryQueryResult {
                id: "q1".to_string(),
                status: MemoryQueryStatus::Ok,
                entries: vec![],
                prompt_block: Some(
                    "[aibe contextual memory]\n[goal]\nship\n[/aibe contextual memory]".into(),
                ),
            }
        }
        other => panic!("unexpected request: {other:?}"),
    });
    let home = tempfile::tempdir().expect("home");
    let cfg = write_ai_config(&server.socket_path, &home);

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AI_SESSION_ID", "phase-a-memory")
        .args(["mem", "show", "--no-start"])
        .output()
        .expect("run ai mem show");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[aibe contextual memory]"));
    assert!(stdout.contains("ship"));
}
