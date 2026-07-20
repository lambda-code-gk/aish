#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use aibe_protocol::{AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut};
use serde_json::Value;

struct MultiSocketServer {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    requests: Arc<Mutex<Vec<Value>>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MultiSocketServer {
    fn spawn(
        count: usize,
        responder: impl Fn(&ClientRequest) -> ClientResponse + Send + Sync + 'static,
    ) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_thread = Arc::clone(&requests);
        let responder = Arc::new(responder);
        let handle = thread::spawn(move || {
            let mut handled = 0usize;
            while handled < count {
                let (stream, _) = listener.accept().expect("accept");
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request");
                let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
                match req {
                    ClientRequest::Ping { .. } => {
                        let response = ClientResponse::Pong {
                            id: "health".to_string(),
                        };
                        let payload = serde_json::to_string(&response).expect("serialize response");
                        writeln!(writer, "{payload}").expect("write response");
                    }
                    _ => {
                        let req_json = serde_json::to_value(&req).expect("to value");
                        requests_thread.lock().expect("lock").push(req_json);
                        let response = responder(&req);
                        let payload = serde_json::to_string(&response).expect("serialize response");
                        writeln!(writer, "{payload}").expect("write response");
                        handled += 1;
                    }
                }
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
    socket_path: &Path,
    history_dir: &Path,
    default_profile: &str,
    tools: &str,
    log_tail_bytes: usize,
    dir: &tempfile::TempDir,
) -> PathBuf {
    let config_path = dir.path().join("ai.toml");
    fs::write(
        &config_path,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
log_tail_bytes = {}
[ask]
default_profile = "{}"
tools = "{}"
"#,
            socket_path.display(),
            history_dir.display(),
            log_tail_bytes,
            default_profile,
            tools,
        ),
    )
    .expect("write config");
    config_path
}

fn setup_session_dir(root: &tempfile::TempDir, tail: &str) -> PathBuf {
    let session = root.path().join("002f15d02b54");
    fs::create_dir(&session).expect("mkdir");
    fs::write(session.join("log.jsonl"), tail).expect("log");
    symlink("log.jsonl", session.join("current_log")).expect("symlink");
    session
}

fn request_content(req: &Value) -> &str {
    req["messages"][0]["content"]
        .as_str()
        .expect("message content")
}

fn request_cwd(req: &Value) -> Option<&str> {
    req["context"]["cwd"].as_str()
}

fn request_shell_log_tail(req: &Value) -> Option<&str> {
    req["context"]["shell_log_tail"].as_str()
}

#[test]
fn history_retry_and_rerun_use_their_expected_sources() {
    let server = MultiSocketServer::spawn(4, |_| ClientResponse::AgentTurnResult {
        id: "turn-1".to_string(),
        status: AgentTurnStatus::Ok,
        assistant_message: ProtocolMessageOut {
            role: "assistant".to_string(),
            content: "ok".to_string(),
        },
        tool_calls: vec![],
        completion_report: None,
    });

    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join(".local/share/ai/history");
    let session_root = tempfile::tempdir().expect("session_root");
    let session_dir = setup_session_dir(&session_root, "tail-one\n");
    let ask_cwd = home.path().join("ask-cwd");
    let rerun_cwd = home.path().join("rerun-cwd");
    fs::create_dir_all(&ask_cwd).expect("ask cwd");
    fs::create_dir_all(&rerun_cwd).expect("rerun cwd");

    let cfg = write_ai_config(
        &server.socket_path,
        &history_dir,
        "fast",
        "@read-only",
        4096,
        &home,
    );

    let ask = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&ask_cwd)
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["--quiet", "--no-start", "hello"])
        .output()
        .expect("run ask");
    assert!(
        ask.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&ask.stderr)
    );

    let index_path = history_dir.join("index.jsonl");
    let index_raw = fs::read_to_string(&index_path).expect("index");
    assert!(
        !index_raw.contains("hello"),
        "index must not contain raw message"
    );
    let index_entry: Value = serde_json::from_str(index_raw.trim()).expect("index json");
    let history_id = index_entry["history_id"]
        .as_str()
        .expect("history id")
        .to_string();
    assert_eq!(index_entry["session_id"], "002f15d02b54");
    assert_eq!(index_entry["profile"], "fast");
    let payload_path = history_dir
        .join("payloads")
        .join(format!("{history_id}.json"));
    let payload_raw = fs::read_to_string(&payload_path).expect("payload");
    assert!(payload_raw.contains("hello"));
    let payload_meta = fs::metadata(&payload_path).expect("payload metadata");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(payload_meta.permissions().mode() & 0o777, 0o600);
    }

    fs::write(session_dir.join("log.jsonl"), "tail-two\n").expect("rewrite session log");

    let history = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&rerun_cwd)
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["history", "--quiet", "--format", "json"])
        .output()
        .expect("run history");
    assert!(
        history.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&history.stderr)
    );
    let history_json: Value = serde_json::from_slice(&history.stdout).expect("history json");
    assert_eq!(history_json.as_array().expect("array").len(), 1);
    assert_eq!(history_json[0]["history_id"], history_id);

    let second_ask = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&ask_cwd)
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env_remove("AISH_SESSION_DIR")
        .env_remove("AI_ASK_LOG")
        .args(["--quiet", "--no-start", "world"])
        .output()
        .expect("run second ask");
    assert!(
        second_ask.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&second_ask.stderr)
    );

    let history_all = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&rerun_cwd)
        .env("AI_CONFIG", &cfg)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["history", "--quiet", "--format", "json"])
        .output()
        .expect("run history all");
    assert!(
        history_all.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&history_all.stderr)
    );
    let history_all_json: Value =
        serde_json::from_slice(&history_all.stdout).expect("history json");
    assert_eq!(history_all_json.as_array().expect("array").len(), 2);

    let retry_config = write_ai_config(
        &server.socket_path,
        &history_dir,
        "slow",
        "none",
        4096,
        &home,
    );

    let retry = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&rerun_cwd)
        .env("AI_CONFIG", &retry_config)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["retry", "--quiet", "--no-start", &history_id])
        .output()
        .expect("run retry");
    assert!(
        retry.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&retry.stderr)
    );

    let rerun = Command::new(env!("CARGO_BIN_EXE_ai"))
        .current_dir(&rerun_cwd)
        .env("AI_CONFIG", &retry_config)
        .env("HOME", home.path())
        .env("AISH_SESSION_DIR", &session_dir)
        .env("AI_ASK_LOG", "session")
        .args(["rerun", "--quiet", "--no-start", &history_id])
        .output()
        .expect("run rerun");
    assert!(
        rerun.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&rerun.stderr)
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    let ask_cwd_str = ask_cwd.display().to_string();
    let rerun_cwd_str = rerun_cwd.display().to_string();

    assert_eq!(request_content(&requests[0]), "hello");
    assert_eq!(request_cwd(&requests[0]), Some(ask_cwd_str.as_str()));
    assert_eq!(request_shell_log_tail(&requests[0]), Some("tail-one\n"));
    assert_eq!(requests[0]["tools"].as_array().expect("tools").len(), 5);
    assert_eq!(requests[0]["llm_profile"], "fast");

    assert_eq!(request_content(&requests[1]), "world");
    assert_eq!(request_cwd(&requests[1]), Some(ask_cwd_str.as_str()));
    assert_eq!(request_shell_log_tail(&requests[1]), None);
    assert_eq!(requests[1]["tools"].as_array().expect("tools").len(), 5);
    assert_eq!(requests[1]["llm_profile"], "fast");

    assert_eq!(request_content(&requests[2]), "hello");
    assert_eq!(request_cwd(&requests[2]), Some(rerun_cwd_str.as_str()));
    assert_eq!(request_shell_log_tail(&requests[2]), Some("tail-two\n"));
    assert_eq!(requests[2]["tools"].as_array().expect("tools").len(), 0);
    assert_eq!(requests[2]["llm_profile"], "slow");

    assert_eq!(request_content(&requests[3]), "hello");
    assert_eq!(request_cwd(&requests[3]), Some(ask_cwd_str.as_str()));
    assert_eq!(request_shell_log_tail(&requests[3]), Some("tail-one\n"));
    assert_eq!(requests[3]["tools"].as_array().expect("tools").len(), 5);
    assert_eq!(requests[3]["llm_profile"], "fast");
}
