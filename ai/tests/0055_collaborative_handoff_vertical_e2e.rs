#![cfg(unix)]
//! 0055 full vertical PTY E2E: `ai` → mock aibe → human handoff → `aish` → PTY shell。

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, HandoffExecutionOutcome, ProtocolMessageOut,
    RequestedCommandCompletion, ShellExecApprovalOrigin,
};

struct DiagnosticLog {
    mock_requests: Vec<String>,
    mock_responses: Vec<String>,
    handoff_result_json: Option<String>,
    ai_stderr: String,
    aish_transcript_hint: String,
    child_exit: Option<i32>,
}

struct CollaborativeMockServer {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
    log: Arc<Mutex<DiagnosticLog>>,
    marker: PathBuf,
}

impl CollaborativeMockServer {
    fn spawn(marker: PathBuf) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let log = Arc::new(Mutex::new(DiagnosticLog {
            mock_requests: Vec::new(),
            mock_responses: Vec::new(),
            handoff_result_json: None,
            ai_stderr: String::new(),
            aish_transcript_hint: String::new(),
            child_exit: None,
        }));
        let log_thread = Arc::clone(&log);
        let marker_thread = marker.clone();
        let handle = thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read agent_turn");
            log_thread
                .lock()
                .expect("log")
                .mock_requests
                .push(line.clone());
            let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse agent_turn");
            let ClientRequest::AgentTurn {
                id,
                context,
                messages,
                ..
            } = req
            else {
                panic!("expected agent_turn");
            };
            assert!(
                context.collaborative_handoff,
                "collaborative_handoff must be true"
            );
            assert!(
                messages.iter().any(|m| m.role == "user"),
                "user message required"
            );

            let prompt = ClientResponse::ShellExecApprovalPrompt {
                id: "handoff-prompt-1".into(),
                turn_id: id.clone(),
                tool_call_id: "call_handoff".into(),
                command: "touch".into(),
                args: vec![marker_thread.to_string_lossy().into_owned()],
            };
            let prompt_json = serde_json::to_string(&prompt).expect("prompt json");
            log_thread
                .lock()
                .expect("log")
                .mock_responses
                .push(prompt_json.clone());
            writeln!(writer, "{prompt_json}").expect("write prompt");
            writer.flush().expect("flush");

            line.clear();
            reader.read_line(&mut line).expect("read approval");
            log_thread
                .lock()
                .expect("log")
                .mock_requests
                .push(line.clone());
            let approval: ClientRequest =
                serde_json::from_str(line.trim()).expect("parse approval");
            let ClientRequest::ShellExecApproval {
                approved,
                approval_origin,
                handoff_result,
                handoff_error,
                ..
            } = approval
            else {
                panic!("expected shell_exec_approval");
            };
            assert!(approved, "handoff must succeed");
            assert_eq!(
                approval_origin,
                ShellExecApprovalOrigin::CollaborativeHandoff
            );
            assert!(handoff_error.is_none());
            let handoff = handoff_result.expect("handoff_result");
            assert_eq!(
                handoff.execution_outcome,
                HandoffExecutionOutcome::HumanControlReturned
            );
            assert_eq!(
                handoff.requested_command_completion,
                RequestedCommandCompletion::Unknown
            );
            log_thread.lock().expect("log").handoff_result_json =
                Some(serde_json::to_string(&handoff).expect("handoff json"));

            let final_resp = ClientResponse::AgentTurnResult {
                id,
                status: AgentTurnStatus::Ok,
                assistant_message: ProtocolMessageOut {
                    role: "assistant".into(),
                    content: "handoff complete".into(),
                },
                tool_calls: vec![],
            };
            let final_json = serde_json::to_string(&final_resp).expect("final json");
            log_thread
                .lock()
                .expect("log")
                .mock_responses
                .push(final_json.clone());
            writeln!(writer, "{final_json}").expect("write final");
            writer.flush().expect("flush");
        });
        Self {
            socket_path,
            _dir: dir,
            handle: Some(handle),
            log,
            marker,
        }
    }

    fn join(mut self) -> DiagnosticLog {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        Arc::try_unwrap(self.log)
            .ok()
            .and_then(|m| m.into_inner().ok())
            .expect("log mutex")
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn write_ai_config(home: &Path, socket_path: &Path, history_dir: &Path) -> PathBuf {
    let path = home.join("ai.toml");
    fs::write(
        &path,
        format!(
            r#"
socket_path = "{}"
history_dir = "{}"
history_max_entries = 0
[ask]
tools = "shell_exec"
progress = false
"#,
            socket_path.display(),
            history_dir.display(),
        ),
    )
    .expect("write ai config");
    path
}

fn write_aibe_config(home: &Path) -> PathBuf {
    let path = home.join("aibe.toml");
    fs::write(
        &path,
        r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#,
    )
    .expect("write aibe config");
    path
}

fn fail_with_diagnostics(context: &str, log: DiagnosticLog) -> ! {
    eprintln!("=== 0055 vertical E2E failure: {context} ===");
    eprintln!("--- ai stderr ---\n{}", log.ai_stderr);
    eprintln!("--- aish transcript hint ---\n{}", log.aish_transcript_hint);
    eprintln!("--- mock aibe requests ---");
    for line in &log.mock_requests {
        eprintln!("{line}");
    }
    eprintln!("--- mock aibe responses ---");
    for line in &log.mock_responses {
        eprintln!("{line}");
    }
    if let Some(json) = &log.handoff_result_json {
        eprintln!("--- handoff result JSON ---\n{json}");
    }
    eprintln!("--- child exit status --- {:?}", log.child_exit);
    panic!("0055 vertical E2E failed: {context}");
}

fn require_test_binary(env_key: &str, fallback_name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(env_key) {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "{env_key} points to missing file: {}",
            path.display()
        );
        return path;
    }
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let sibling = ai_bin
        .parent()
        .map(|dir| dir.join(fallback_name))
        .unwrap_or_else(|| PathBuf::from(fallback_name));
    assert!(
        sibling.is_file(),
        "{fallback_name} binary not found (set {env_key} or build aish)"
    );
    sibling
}

const FORBIDDEN_PERSISTENT_NAMES: &[&str] = &[
    "handoffs",
    "handoff.json",
    "workflow.json",
    "checkpoint.json",
    "lease.json",
    "side-run-lock.json",
    "candidates.jsonl",
    "shell_sessions.jsonl",
];

fn forbidden_snapshot(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    collect_forbidden(root, &mut found);
    found.sort();
    found
}

fn collect_forbidden(dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if FORBIDDEN_PERSISTENT_NAMES.contains(&name.as_str()) {
            out.push(path.display().to_string());
        }
        if path.is_dir() {
            collect_forbidden(&path, out);
        }
    }
}

fn write_aish_config(home: &Path, log_dir: &Path) -> PathBuf {
    let path = home.join("aish.toml");
    fs::write(
        &path,
        format!(
            "log_dir = \"{}\"\n",
            log_dir.display().to_string().replace('\\', "\\\\")
        ),
    )
    .expect("write aish config");
    path
}

fn assert_runtime_handoff_cleaned(home: &Path) {
    let aish_runtime = home.join("aish");
    if !aish_runtime.is_dir() {
        return;
    }
    for entry in fs::read_dir(&aish_runtime).expect("read runtime root") {
        let name = entry.expect("entry").file_name();
        let n = name.to_string_lossy();
        assert!(
            !n.starts_with("handoff-"),
            "runtime handoff directory must be removed after success: {}",
            aish_runtime.join(&*n).display()
        );
    }
}

fn run_collaborative_handoff(
    ai_bin: &Path,
    aish_bin: &Path,
    home: &Path,
    work: &Path,
    shell_input: &[u8],
) -> (std::process::Output, CollaborativeMockServer) {
    let marker = work.join("handoff-marker");
    let server = CollaborativeMockServer::spawn(marker);
    let history_dir = home.join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let ai_cfg = write_ai_config(home, &server.socket_path, &history_dir);
    let aibe_cfg = write_aibe_config(home);
    let aish_log = home.join("aish-sessions");
    let aish_cfg = write_aish_config(home, &aish_log);

    let mut child = Command::new(ai_bin)
        .args([
            "ask",
            "--collaborative",
            "--quiet",
            "--no-start",
            "verify collaborative handoff",
        ])
        .current_dir(work)
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("AISH_CONFIG", &aish_cfg)
        .env("HOME", home)
        .env("AISH_BIN", aish_bin)
        .env("XDG_RUNTIME_DIR", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ai");
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(shell_input)
            .expect("write shell input to ai stdin");
    }
    let output = child.wait_with_output().expect("wait ai");
    (output, server)
}

#[test]
fn no_persistent_handoff_state_is_created() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let aish_bin = require_test_binary("CARGO_BIN_EXE_aish", "aish");

    let home = tempfile::tempdir().expect("home");
    let aibe_root = home.path().join(".local/share/aibe");
    let aish_log = home.path().join("aish-sessions");
    fs::create_dir_all(&aibe_root).expect("aibe root");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");

    let before_aibe = forbidden_snapshot(&aibe_root);
    let before_aish = forbidden_snapshot(&aish_log);

    let (output, server) =
        run_collaborative_handoff(&ai_bin, &aish_bin, home.path(), &work, b"exit\n");
    let mut log = server.join();
    log.ai_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log.child_exit = output.status.code();

    if !output.status.success() {
        fail_with_diagnostics("ai exited with failure", log);
    }
    if log.handoff_result_json.is_none() {
        fail_with_diagnostics("mock aibe never received handoff_result", log);
    }

    let after_aibe = forbidden_snapshot(&aibe_root);
    let after_aish = forbidden_snapshot(&aish_log);
    assert_eq!(
        before_aibe, after_aibe,
        "forbidden persistent files appeared under aibe root"
    );
    assert_eq!(
        before_aish, after_aish,
        "forbidden persistent files appeared under aish log root"
    );
    assert_runtime_handoff_cleaned(home.path());
}

#[test]
fn collaborative_handoff_full_vertical_pty_e2e() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let aish_bin = require_test_binary("CARGO_BIN_EXE_aish", "aish");
    assert!(
        ai_bin.is_file(),
        "ai binary missing at {}",
        ai_bin.display()
    );

    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work dir");
    let marker = work.join("human-created-marker");
    assert!(!marker.exists(), "marker must not exist before handoff");

    let server = CollaborativeMockServer::spawn(marker.clone());
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let _ai_cfg = write_ai_config(home.path(), &server.socket_path, &history_dir);
    let _aibe_cfg = write_aibe_config(home.path());

    let shell_input = format!(
        "test -f {} && exit 1\n touch {}\n exit\n",
        shell_quote(marker.to_str().unwrap()),
        shell_quote(marker.to_str().unwrap()),
    );

    let (output, server) = run_collaborative_handoff(
        &ai_bin,
        &aish_bin,
        home.path(),
        &work,
        shell_input.as_bytes(),
    );
    let mut log = server.join();
    log.ai_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log.child_exit = output.status.code();
    log.aish_transcript_hint = log.handoff_result_json.clone().unwrap_or_default();

    if !output.status.success() {
        fail_with_diagnostics("ai exited with failure", log);
    }
    if !marker.exists() {
        fail_with_diagnostics("human did not create marker file", log);
    }
    if log.handoff_result_json.is_none() {
        fail_with_diagnostics("mock aibe never received handoff_result", log);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("handoff complete"),
        "unexpected stdout: {stdout}"
    );
}
