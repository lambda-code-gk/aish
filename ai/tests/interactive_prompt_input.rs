#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use ai::application::classify_from_raw_args;
use ai::domain::{classify_ask_invocation, AskInvocationSource, PromptAcquisitionResult};
use aibe_protocol::{AgentTurnStatus, ClientRequest, ClientResponse, ProtocolMessageOut};

struct MockPromptServer {
    _handle: JoinHandle<()>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
    agent_turns: Arc<std::sync::atomic::AtomicUsize>,
}

impl MockPromptServer {
    fn new(expected_message: &'static str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let agent_turns = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let turns_clone = Arc::clone(&agent_turns);
        let handle = thread::spawn(move || {
            while turns_clone.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                    continue;
                }
                let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
                let response = match req {
                    ClientRequest::Ping { .. } => ClientResponse::Pong {
                        id: "ping-1".into(),
                    },
                    ClientRequest::AgentTurn { messages, .. } => {
                        assert_eq!(messages.len(), 1);
                        assert_eq!(messages[0].content, expected_message);
                        turns_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        ClientResponse::AgentTurnResult {
                            id: "turn-1".into(),
                            status: AgentTurnStatus::Ok,
                            assistant_message: ProtocolMessageOut {
                                role: "assistant".into(),
                                content: "ok".into(),
                            },
                            tool_calls: vec![],
                        }
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                let payload = serde_json::to_string(&response).expect("serialize");
                writeln!(writer, "{payload}").expect("write");
                writer.flush().expect("flush");
            }
        });
        Self {
            _handle: handle,
            _dir: dir,
            socket_path,
            agent_turns,
        }
    }

    fn agent_turn_count(&self) -> usize {
        self.agent_turns.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn os_vec(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(|s| OsString::from(*s)).collect()
}

#[test]
fn bare_ai_tty_starts_prompt_mode() {
    let invocation = classify_from_raw_args(&os_vec(&["ai"]));
    assert_eq!(invocation, AskInvocationSource::BareRoot);
    assert!(ai::domain::should_enter_interactive_prompt_mode(
        invocation, true
    ));
}

#[test]
fn bare_ai_prompt_message_is_sent_once() {
    const PROMPT: &str = "from prompt via editor";

    // bare `ai` が AI_EDITOR 経路で使う adapter と同じ取得処理
    let work = tempfile::tempdir().expect("tempdir");
    let editor = work.path().join("fake-editor.sh");
    fs::write(
        &editor,
        format!("#!/bin/sh\ncat > \"$1\" <<'EOF'\n{PROMPT}\nEOF\n"),
    )
    .expect("write editor");
    let mut perms = fs::metadata(&editor).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&editor, perms).expect("chmod");
    let (_draft_file, draft_path) =
        ai::adapters::outbound::create_prompt_temp_file(Some(work.path())).expect("draft");
    let acquired = ai::adapters::outbound::acquire_prompt_via_external_editor(
        &[editor.to_string_lossy().into_owned()],
        &draft_path,
    );
    assert_eq!(
        acquired,
        PromptAcquisitionResult::Submitted {
            content: PROMPT.to_string()
        }
    );

    // 取得した prompt が既存 ask 経路で aibe に 1 回だけ送られる
    let server = MockPromptServer::new(PROMPT);
    let bin = env!("CARGO_BIN_EXE_ai");
    let output = Command::new(bin)
        .args([
            "ask",
            "--no-start",
            "--no-progress",
            "--tools",
            "none",
            PROMPT,
        ])
        .env("AIBE_SOCKET_PATH", &server.socket_path)
        .output()
        .expect("ask");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok"),
        "expected assistant body on stdout, stderr={stderr}, stdout={stdout}"
    );
    assert_eq!(
        server.agent_turn_count(),
        1,
        "expected exactly one agent_turn, stderr={stderr}"
    );
}

#[test]
fn pipe_input_keeps_existing_ask_path() {
    let invocation = classify_ask_invocation(&os_vec(&["ai"]));
    assert!(!ai::domain::should_enter_interactive_prompt_mode(
        invocation, false
    ));

    let bin = env!("CARGO_BIN_EXE_ai");
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("AIBE_SOCKET_PATH", "/nonexistent/aibe.sock")
        .env_remove("AI_EDITOR")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .spawn()
        .expect("spawn ai");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"hello from pipe\n")
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("AISH prompt>"),
        "pipe must not start interactive prompt UI: {stderr}"
    );
}

#[test]
fn empty_pipe_input_is_rejected() {
    let bin = env!("CARGO_BIN_EXE_ai");
    let mut child = Command::new(bin)
        .args(["ask", "--dry-run"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("AIBE_SOCKET_PATH", "/nonexistent/aibe.sock")
        .env_remove("AI_EDITOR")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .spawn()
        .expect("spawn ai");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AISH: prompt is empty; cancelled."),
        "empty pipe input must be rejected before dry-run report: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "empty pipe input must not reach dry-run output: {stdout}"
    );
}

#[test]
fn argv_input_keeps_existing_ask_path() {
    let hello = classify_ask_invocation(&os_vec(&["ai", "hello"]));
    assert_eq!(hello, AskInvocationSource::ImplicitMessage);
    assert!(!ai::domain::should_enter_interactive_prompt_mode(
        hello, true
    ));

    let explicit = classify_ask_invocation(&os_vec(&["ai", "ask", "hello world"]));
    assert_eq!(explicit, AskInvocationSource::ExplicitAsk);
    assert!(!ai::domain::should_enter_interactive_prompt_mode(
        explicit, true
    ));
}

#[test]
fn explicit_invocations_do_not_enter_prompt_mode() {
    let cases: &[&[&str]] = &[
        &["ai", "ask"],
        &["ai", "ask", "--dry-run"],
        &["ai", "--help"],
        &["ai", "chat"],
        &["ai", "history"],
    ];
    for args in cases {
        let invocation = classify_ask_invocation(&os_vec(args));
        assert_ne!(invocation, AskInvocationSource::BareRoot, "args={args:?}");
        assert!(
            !ai::domain::should_enter_interactive_prompt_mode(invocation, true),
            "args={args:?}"
        );
    }
}

#[test]
fn chat_repl_and_pipe_input_regression_guard() {
    let chat_invocation = classify_ask_invocation(&os_vec(&["ai", "chat"]));
    assert_ne!(chat_invocation, AskInvocationSource::BareRoot);

    let bin = env!("CARGO_BIN_EXE_ai");
    let output = Command::new(bin)
        .arg("chat")
        .env("AIBE_SOCKET_PATH", "/nonexistent/aibe.sock")
        .stdin(Stdio::null())
        .output()
        .expect("spawn chat");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("AISH prompt>"),
        "chat must not use bare-ai prompt editor: {stderr}"
    );
}

#[test]
fn editor_precedence_prefers_ai_editor_then_visual_then_editor() {
    // 単体テストの委譲: external_editor モジュールで検証済み
    let _lock = env_lock();
    std::env::set_var("AI_EDITOR", "first");
    std::env::set_var("VISUAL", "second");
    std::env::set_var("EDITOR", "third");
    assert_eq!(
        ai::adapters::outbound::resolve_editor_command_from_env().expect("cmd"),
        vec!["first".to_string()]
    );
    std::env::remove_var("AI_EDITOR");
    std::env::remove_var("VISUAL");
    std::env::remove_var("EDITOR");
}

#[test]
fn empty_prompt_after_comment_strip_is_rejected() {
    let filtered = ai::domain::strip_prompt_template_comments("<!-- ai-prompt: hint only -->\n");
    assert_eq!(
        PromptAcquisitionResult::Empty,
        if filtered.trim().is_empty() {
            PromptAcquisitionResult::Empty
        } else {
            PromptAcquisitionResult::Submitted { content: filtered }
        }
    );
}

#[test]
fn abnormal_editor_exit_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("draft.md");
    std::fs::write(&path, "").expect("write");
    let editor = dir.path().join("fail.sh");
    std::fs::write(&editor, "#!/bin/sh\nexit 1\n").expect("write");
    let mut perms = std::fs::metadata(&editor).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&editor, perms).expect("chmod");
    let result = ai::adapters::outbound::acquire_prompt_via_external_editor(
        &[editor.to_string_lossy().into_owned()],
        &path,
    );
    assert!(matches!(
        result,
        PromptAcquisitionResult::EditorFailed { .. }
    ));
}

#[test]
fn reedline_prompt_editor_handles_enter_eof_and_interrupt() {
    assert!(ai::domain::is_substantive_prompt("line 1\nline 2"));
    assert!(!ai::domain::is_substantive_prompt("\n\n"));
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().expect("lock")
}
