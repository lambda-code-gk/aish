#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use aibe_protocol::{AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut};
use serde_json::Value;

struct MultiSocketServer {
    socket_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
    requests: Arc<Mutex<Vec<Value>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MultiSocketServer {
    fn spawn(
        count: usize,
        responder: impl Fn(&ClientRequest, usize) -> ClientResponse + Send + Sync + 'static,
    ) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_thread = Arc::clone(&requests);
        let responder = Arc::new(responder);
        let turn = Arc::new(AtomicUsize::new(0));
        let handle = thread::spawn(move || {
            let mut handled = 0usize;
            while handled < count {
                let (stream, _) = listener.accept().expect("accept");
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request");
                let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
                let req_json = serde_json::to_value(&req).expect("to value");
                requests_thread.lock().expect("lock").push(req_json);
                let n = turn.fetch_add(1, Ordering::SeqCst);
                let response = responder(&req, n);
                let payload = serde_json::to_string(&response).expect("serialize response");
                writeln!(writer, "{payload}").expect("write response");
                handled += 1;
            }
        });

        Self {
            socket_path,
            _dir: dir,
            requests,
            handle: Some(handle),
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("lock").clone()
    }
}

impl Drop for MultiSocketServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_ai_config(
    home: &std::path::Path,
    socket_path: &std::path::Path,
    history_dir: &std::path::Path,
) -> std::path::PathBuf {
    let config_path = home.join("ai.toml");
    fs::write(
        &config_path,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
[ask]
default_profile = "fast"
"#,
            socket_path.display(),
            history_dir.display(),
        ),
    )
    .expect("write config");
    config_path
}
#[test]
fn chat_dry_run_skips_aibe() {
    let home = tempfile::tempdir().expect("home");
    let cfg_path = home.path().join("ai.toml");
    fs::write(&cfg_path, "socket_path = \"/tmp/does-not-exist.sock\"\n").expect("write config");

    let out = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg_path)
        .env("HOME", home.path())
        .args(["chat", "--quiet", "--dry-run", "--format", "json"])
        .output()
        .expect("run ai chat dry-run");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["command"], "chat");
    assert_eq!(json["message_source"], "repl");
    assert_eq!(json["message_masked"], "<masked>");
}

#[test]
fn chat_accumulates_transcript_and_records_conversation_id() {
    let server = MultiSocketServer::spawn(2, |req, turn| {
        if let ClientRequest::AgentTurn { messages, .. } = req {
            match turn {
                0 => {
                    assert_eq!(messages.len(), 1);
                    assert_eq!(messages[0].content, "hello");
                }
                1 => {
                    assert_eq!(messages.len(), 3);
                    assert_eq!(messages[0].content, "hello");
                    assert_eq!(messages[1].content, "reply1");
                    assert_eq!(messages[2].content, "world");
                }
                other => panic!("unexpected turn {other}"),
            }
        } else {
            panic!("unexpected request: {req:?}");
        }
        let content = if turn == 0 { "reply1" } else { "reply2" };
        ClientResponse::AgentTurnResult {
            id: format!("turn-{turn}"),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".to_string(),
                content: content.to_string(),
            },
            tool_calls: vec![],
        }
    });

    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let cfg = write_ai_config(home.path(), &server.socket_path, &history_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["chat", "--quiet", "--no-start"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ai chat");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello\nworld\n/exit\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(server.requests().len(), 2);

    let history = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args([
            "history",
            "--quiet",
            "--format",
            "json",
            "--command",
            "chat",
        ])
        .output()
        .expect("history");
    assert!(history.status.success());
    let entries: Vec<Value> = serde_json::from_slice(&history.stdout).expect("history json");
    assert_eq!(entries.len(), 2);
    let conv0 = entries[0]["conversation_id"].as_str().expect("conv0");
    let conv1 = entries[1]["conversation_id"].as_str().expect("conv1");
    assert!(!conv0.is_empty());
    assert_eq!(conv0, conv1);
}

#[test]
fn chat_rerun_restores_saved_transcript() {
    let server = MultiSocketServer::spawn(3, |req, turn| {
        if turn == 2 {
            if let ClientRequest::AgentTurn { messages, .. } = req {
                assert_eq!(messages.len(), 3);
                assert_eq!(messages[0].content, "hello");
                assert_eq!(messages[1].content, "reply1");
                assert_eq!(messages[2].content, "world");
            } else {
                panic!("unexpected request on rerun: {req:?}");
            }
        }
        let content = match turn {
            0 => "reply1",
            1 => "reply2",
            2 => "reply3",
            _ => panic!("unexpected turn {turn}"),
        };
        ClientResponse::AgentTurnResult {
            id: format!("turn-{turn}"),
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".to_string(),
                content: content.to_string(),
            },
            tool_calls: vec![],
        }
    });

    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let cfg = write_ai_config(home.path(), &server.socket_path, &history_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["chat", "--quiet", "--no-start"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ai chat");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello\nworld\n/exit\n")
        .expect("write stdin");
    assert!(child.wait().expect("wait").success());

    let history = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args([
            "history",
            "--quiet",
            "--format",
            "json",
            "--command",
            "chat",
        ])
        .output()
        .expect("history");
    let entries: Vec<Value> = serde_json::from_slice(&history.stdout).expect("history json");
    assert_eq!(entries.len(), 2);
    let second_turn_id = entries[0]["history_id"]
        .as_str()
        .expect("history_id")
        .to_string();

    let rerun = Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .args(["rerun", "--quiet", "--no-start", &second_turn_id])
        .output()
        .expect("rerun");
    assert!(
        rerun.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    assert_eq!(server.requests().len(), 3);
}
