#![cfg(unix)]
//! 0055 transport E2E: `ai` → mock aibe → human handoff → `aish` → PTY shell。
//! 実 aibe server の ShellExecTool 境界は通さない（aibe 側 integration test で担保）。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, ErrorCode, HandoffExecutionOutcome,
    ProtocolMessageOut, RequestedCommandCompletion, RouteKind, RoutePlan, RouteTurnStatus,
    ShellExecApprovalOrigin,
};

fn e2e_timeout() -> Duration {
    let secs = std::env::var("AISH_0055_E2E_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    Duration::from_secs(secs.min(60))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureFinalResponse {
    ToolError,
    SuccessTurnResult,
    HoldConnectionOpen,
    CloseConnection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedHandoffDecision {
    Success,
    Failure(FailureFinalResponse),
}

struct DiagnosticLog {
    mock_requests: Vec<String>,
    mock_responses: Vec<String>,
    handoff_result_json: Option<String>,
    handoff_error_json: Option<String>,
    held_connection_without_final_response: bool,
    connection_closed_by_client: bool,
    ai_stderr: String,
    aish_transcript_hint: String,
    child_exit: Option<i32>,
}

struct CollaborativeMockServer {
    socket_path: PathBuf,
    _dir: tempfile::TempDir,
    handle: Option<JoinHandle<()>>,
    listener: Option<UnixListener>,
    log: Arc<Mutex<DiagnosticLog>>,
    candidate_marker: PathBuf,
    expected: ExpectedHandoffDecision,
}

impl CollaborativeMockServer {
    fn spawn(candidate_marker: PathBuf, expected: ExpectedHandoffDecision) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("aibe.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let log = Arc::new(Mutex::new(DiagnosticLog {
            mock_requests: Vec::new(),
            mock_responses: Vec::new(),
            handoff_result_json: None,
            handoff_error_json: None,
            held_connection_without_final_response: false,
            connection_closed_by_client: false,
            ai_stderr: String::new(),
            aish_transcript_hint: String::new(),
            child_exit: None,
        }));
        Self {
            socket_path,
            _dir: dir,
            handle: None,
            listener: Some(listener),
            log,
            candidate_marker,
            expected,
        }
    }

    fn start_accept(&mut self) {
        let listener = self.listener.take().expect("listener");
        let log_thread = Arc::clone(&self.log);
        let marker_thread = self.candidate_marker.clone();
        let expected = self.expected;
        listener.set_nonblocking(true).expect("set_nonblocking");
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + e2e_timeout();
            let (stream, _) = accept_with_deadline(&listener, deadline);
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            read_line_with_deadline(&mut reader, &mut line, deadline).expect("read agent_turn");
            log_thread
                .lock()
                .expect("log")
                .mock_requests
                .push(line.clone());
            let mut req: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
            if matches!(req, ClientRequest::RouteTurn { .. }) {
                let response = ClientResponse::RouteTurnResult {
                    id: "route-0055".into(),
                    status: RouteTurnStatus::Ok,
                    plan: RoutePlan {
                        conversation_id: "collab-e2e".into(),
                        new_conversation: true,
                        route_kind: RouteKind::ToolAssisted,
                        recommended_preset: None,
                        recommended_tools: None,
                        log_tail_bytes: None,
                        feature_actions: vec![],
                        require_shell_approval: true,
                        log_tail_escalation: false,
                        route_reason: "collaborative E2E".into(),
                        confidence: None,
                    },
                };
                writeln!(writer, "{}", serde_json::to_string(&response).unwrap()).unwrap();
                writer.flush().unwrap();
                let (next_stream, _) = accept_with_deadline(&listener, deadline);
                writer = next_stream.try_clone().expect("clone agent stream");
                reader = BufReader::new(next_stream);
                line.clear();
                read_line_with_deadline(&mut reader, &mut line, deadline)
                    .expect("read agent_turn after route");
                req = serde_json::from_str(line.trim()).expect("parse agent_turn");
            }
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
            if read_line_with_deadline(&mut reader, &mut line, deadline).is_err() {
                return;
            }
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
            assert_eq!(
                approval_origin,
                ShellExecApprovalOrigin::CollaborativeHandoff
            );

            match expected {
                ExpectedHandoffDecision::Success => {
                    assert!(approved, "handoff must succeed");
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
                    assert_eq!(
                        handoff.collab_outcome.status,
                        aibe_protocol::CollabOutcomeStatus::Done
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
                }
                ExpectedHandoffDecision::Failure(final_response) => {
                    assert!(!approved, "handoff must fail");
                    assert!(handoff_result.is_none());
                    let error = handoff_error.expect("handoff_error");
                    assert_eq!(error.code, "human_handoff_failed");
                    log_thread.lock().expect("log").handoff_error_json =
                        Some(serde_json::to_string(&error).expect("error json"));

                    match final_response {
                        FailureFinalResponse::ToolError => {
                            let final_resp = ClientResponse::Error {
                                id,
                                code: ErrorCode::ToolError,
                                message: format!("human_handoff_failed: {}", error.message),
                            };
                            let final_json =
                                serde_json::to_string(&final_resp).expect("final json");
                            log_thread
                                .lock()
                                .expect("log")
                                .mock_responses
                                .push(final_json.clone());
                            writeln!(writer, "{final_json}").expect("write final");
                            writer.flush().expect("flush");
                        }
                        FailureFinalResponse::SuccessTurnResult => {
                            let final_resp = ClientResponse::AgentTurnResult {
                                id,
                                status: AgentTurnStatus::Ok,
                                assistant_message: ProtocolMessageOut {
                                    role: "assistant".into(),
                                    content: "should not win".into(),
                                },
                                tool_calls: vec![],
                            };
                            let final_json =
                                serde_json::to_string(&final_resp).expect("final json");
                            log_thread
                                .lock()
                                .expect("log")
                                .mock_responses
                                .push(final_json.clone());
                            writeln!(writer, "{final_json}").expect("write final");
                            writer.flush().expect("flush");
                        }
                        FailureFinalResponse::HoldConnectionOpen => {
                            log_thread
                                .lock()
                                .expect("log")
                                .held_connection_without_final_response = true;
                            let stream = reader.get_mut();
                            stream.set_nonblocking(true).expect("set nonblocking");
                            let mut probe = [0_u8; 1];
                            loop {
                                if Instant::now() >= deadline {
                                    panic!("client did not abort handoff before E2E deadline");
                                }
                                match stream.read(&mut probe) {
                                    Ok(0) => {
                                        log_thread
                                            .lock()
                                            .expect("log")
                                            .connection_closed_by_client = true;
                                        break;
                                    }
                                    Ok(_) => {}
                                    Err(error)
                                        if error.kind() == std::io::ErrorKind::WouldBlock =>
                                    {
                                        thread::sleep(Duration::from_millis(20));
                                    }
                                    Err(error) => panic!("connection hold failed: {error}"),
                                }
                            }
                        }
                        FailureFinalResponse::CloseConnection => {}
                    }
                }
            }
        });
        self.handle = Some(handle);
    }

    fn join(self, deadline: Instant) -> DiagnosticLog {
        if let Some(handle) = self.handle {
            join_with_deadline(handle, deadline);
        }
        Arc::try_unwrap(self.log)
            .ok()
            .and_then(|m| m.into_inner().ok())
            .expect("log mutex")
    }
}

fn accept_with_deadline(
    listener: &UnixListener,
    deadline: Instant,
) -> (
    std::os::unix::net::UnixStream,
    std::os::unix::net::SocketAddr,
) {
    loop {
        if Instant::now() >= deadline {
            panic!("mock server accept deadline exceeded");
        }
        match listener.accept() {
            Ok(conn) => return conn,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("mock server accept failed: {e}"),
        }
    }
}

fn read_line_with_deadline(
    reader: &mut BufReader<std::os::unix::net::UnixStream>,
    line: &mut String,
    deadline: Instant,
) -> std::io::Result<()> {
    while Instant::now() < deadline {
        reader.get_mut().set_nonblocking(true)?;
        match reader.read_line(line) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "mock server connection closed by peer",
                ));
            }
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "mock server read_line deadline exceeded",
    ))
}

fn join_with_deadline(handle: JoinHandle<()>, deadline: Instant) {
    while Instant::now() < deadline {
        if handle.is_finished() {
            handle.join().expect("mock aibe server panicked");
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("mock server thread join deadline exceeded");
}

fn spawn_in_new_process_group(command: &mut Command) -> Child {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().expect("spawn child in new process group")
}

fn kill_process_group_and_reap(child: Child) -> std::process::Output {
    let pgid = child.id() as i32;
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    child
        .wait_with_output()
        .expect("wait output after process group kill")
}

fn wait_child_with_timeout(mut child: Child, deadline: Instant) -> std::process::Output {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().expect("wait output"),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let output = kill_process_group_and_reap(child);
                panic!(
                    "ai child timed out after {:?}, stderr={}",
                    e2e_timeout(),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => panic!("wait child failed: {e}"),
        }
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
    eprintln!("=== 0055 transport E2E failure: {context} ===");
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
    if let Some(json) = &log.handoff_error_json {
        eprintln!("--- handoff error JSON ---\n{json}");
    }
    eprintln!("--- child exit status --- {:?}", log.child_exit);
    panic!("0055 transport E2E failed: {context}");
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

struct HandoffRun {
    output: std::process::Output,
    server: CollaborativeMockServer,
    deadline: Instant,
}

fn run_collaborative_handoff(
    ai_bin: &Path,
    aish_bin: &Path,
    home: &Path,
    work: &Path,
    candidate_marker: PathBuf,
    shell_input: &[u8],
) -> HandoffRun {
    run_collaborative_handoff_with_expected(
        ai_bin,
        aish_bin,
        home,
        work,
        candidate_marker,
        shell_input,
        ExpectedHandoffDecision::Success,
    )
}

fn run_collaborative_handoff_with_expected(
    ai_bin: &Path,
    aish_bin: &Path,
    home: &Path,
    work: &Path,
    candidate_marker: PathBuf,
    shell_input: &[u8],
    expected: ExpectedHandoffDecision,
) -> HandoffRun {
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(candidate_marker, expected);
    server.start_accept();
    let history_dir = home.join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let ai_cfg = write_ai_config(home, &server.socket_path, &history_dir);
    let aibe_cfg = write_aibe_config(home);
    let aish_log = home.join("aish-sessions");
    let aish_cfg = write_aish_config(home, &aish_log);

    let mut master = -1;
    let mut slave = -1;
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        },
        0,
        "openpty"
    );
    let slave_file = unsafe { fs::File::from_raw_fd(slave) };
    let mut child = {
        let mut command = Command::new(ai_bin);
        command
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
            .stdin(Stdio::from(slave_file))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().expect("spawn ai with controlling PTY")
    };
    let mut master_file = unsafe { fs::File::from_raw_fd(master) };
    let mut stderr = child.stderr.take().expect("ai stderr");
    let shell_input = shell_input.to_vec();
    let input_driver = thread::spawn(move || {
        let mut transcript = Vec::new();
        let mut byte = [0_u8; 1];
        let mut shell_sent = false;
        let mut outcome_sent = false;
        while stderr.read(&mut byte).unwrap_or(0) != 0 {
            transcript.push(byte[0]);
            let text = String::from_utf8_lossy(&transcript);
            if !shell_sent && text.contains("Press Ctrl+D or run `exit` to return control.") {
                thread::sleep(Duration::from_millis(200));
                master_file
                    .write_all(&shell_input)
                    .expect("write shell input to ai PTY");
                shell_sent = true;
            }
            if !outcome_sent && text.contains("作業結果を選択してください") {
                master_file
                    .write_all(b"d\n")
                    .expect("write outcome to ai PTY");
                outcome_sent = true;
            }
        }
        transcript
    });
    let mut output = wait_child_with_timeout(child, deadline);
    output.stderr = input_driver.join().expect("join PTY input driver");
    HandoffRun {
        output,
        server,
        deadline,
    }
}

fn spawn_ai_collaborative_failure(
    ai_bin: &Path,
    home: &Path,
    work: &Path,
    socket_path: &Path,
    aish_bin: &Path,
    xdg_runtime_dir: &Path,
    prompt: &str,
) -> Child {
    let history_dir = home.join("history");
    fs::create_dir_all(&history_dir).expect("history");
    let ai_cfg = write_ai_config(home, socket_path, &history_dir);
    let aibe_cfg = write_aibe_config(home);
    let aish_cfg = write_aish_config(home, &home.join("aish-sessions"));
    let mut command = Command::new(ai_bin);
    command
        .args(["ask", "--collaborative", "--quiet", "--no-start", prompt])
        .current_dir(work)
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("AISH_CONFIG", &aish_cfg)
        .env("HOME", home)
        .env("AISH_BIN", aish_bin)
        .env("XDG_RUNTIME_DIR", xdg_runtime_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn_in_new_process_group(&mut command)
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

    let run = run_collaborative_handoff(
        &ai_bin,
        &aish_bin,
        home.path(),
        &work,
        work.join("unused-candidate-marker"),
        b"exit\n",
    );
    let mut log = run.server.join(run.deadline);
    log.ai_stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    log.child_exit = run.output.status.code();

    if !run.output.status.success() {
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
fn ai_to_aish_handoff_transport_pty_e2e() {
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
    let candidate_marker = work.join("candidate-marker");
    let human_marker = work.join("human-marker");
    let verdict_file = work.join("verdict.txt");
    assert!(!candidate_marker.exists());
    assert!(!human_marker.exists());
    assert!(!verdict_file.exists());

    let shell_input = format!(
        "if test -e {}; then\n  printf auto_executed > {}\nelse\n  printf not_auto_executed > {}\nfi\n touch {}\n exit\n",
        shell_quote(candidate_marker.to_str().unwrap()),
        shell_quote(verdict_file.to_str().unwrap()),
        shell_quote(verdict_file.to_str().unwrap()),
        shell_quote(human_marker.to_str().unwrap()),
    );

    let run = run_collaborative_handoff(
        &ai_bin,
        &aish_bin,
        home.path(),
        &work,
        candidate_marker.clone(),
        shell_input.as_bytes(),
    );
    let mut log = run.server.join(run.deadline);
    log.ai_stderr = String::from_utf8_lossy(&run.output.stderr).into_owned();
    log.child_exit = run.output.status.code();
    log.aish_transcript_hint = log.handoff_result_json.clone().unwrap_or_default();

    if !run.output.status.success() {
        fail_with_diagnostics("ai exited with failure", log);
    }
    let verdict = fs::read_to_string(&verdict_file).unwrap_or_default();
    if verdict.trim() != "not_auto_executed" {
        fail_with_diagnostics("candidate command appears to have been auto-executed", log);
    }
    if !human_marker.exists() {
        fail_with_diagnostics("human did not create human_marker file", log);
    }
    if candidate_marker.exists() {
        fail_with_diagnostics("candidate_marker must not exist", log);
    }
    if log.handoff_result_json.is_none() {
        fail_with_diagnostics("mock aibe never received handoff_result", log);
    }
    let stdout = String::from_utf8_lossy(&run.output.stdout);
    assert!(
        stdout.contains("handoff complete"),
        "unexpected stdout: {stdout}"
    );
}

#[test]
fn collab_outcome_returns_structured_to_parent() {
    ai_to_aish_handoff_transport_pty_e2e();
}

#[test]
fn handoff_runtime_dir_failure_exits_nonzero() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let aish_bin = require_test_binary("CARGO_BIN_EXE_aish", "aish");
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");

    let blocked = home.path().join("blocked-runtime");
    fs::create_dir_all(&blocked).expect("blocked");
    fs::write(blocked.join("aish"), b"not-a-directory").expect("block aish root");

    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::ToolError),
    );
    server.start_accept();

    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &aish_bin,
            &blocked,
            "verify handoff failure",
        ),
        deadline,
    );
    let log = server.join(deadline);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "handoff runtime failure must exit non-zero, stderr={stderr}"
    );
    assert_ne!(
        output.status.code(),
        Some(130),
        "handoff failure must not look like SIGINT"
    );
    assert!(
        stderr.contains("human_handoff_failed") || stderr.contains("handoff runtime dir"),
        "stderr must mention handoff failure: {stderr}"
    );
    assert!(
        !stderr.contains("shell_exec rejected by user"),
        "handoff failure must not be reported as user denial"
    );
    assert!(
        log.handoff_error_json.is_some(),
        "mock server must receive structured handoff_error"
    );
    assert!(
        log.handoff_error_json
            .as_deref()
            .is_some_and(|json| json.contains("human_handoff_failed")),
        "handoff_error.code must be human_handoff_failed"
    );
}

#[test]
fn handoff_launcher_failure_is_not_sigint() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::ToolError),
    );
    server.start_accept();

    let missing_aish = work.join("missing-aish-binary");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify launcher failure",
        ),
        deadline,
    );
    let log = server.join(deadline);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(130));
    assert!(
        stderr.contains("human_handoff_failed") || stderr.contains("collaborative handoff failed"),
        "stderr={stderr}"
    );
    assert!(
        log.handoff_error_json.is_some(),
        "mock server must receive structured handoff_error"
    );
}

#[test]
fn handoff_failure_cannot_finish_as_success() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::SuccessTurnResult),
    );
    server.start_accept();

    let missing_aish = work.join("missing-aish");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify cannot succeed",
        ),
        deadline,
    );
    let log = server.join(deadline);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("handoff complete"),
        "handoff failure must not finish as successful assistant output"
    );
    assert!(
        !stdout.contains("should not win"),
        "handoff failure must not finish as successful assistant output"
    );
    assert!(log.handoff_error_json.is_some());
}

#[test]
fn handoff_failure_aborts_when_server_sends_no_final_response() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::HoldConnectionOpen),
    );
    server.start_accept();

    let missing_aish = work.join("missing-aish");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify abort without final response",
        ),
        deadline,
    );
    let log = server.join(deadline);
    assert!(
        !output.status.success(),
        "handoff failure must exit non-zero even without server final response"
    );
    assert_ne!(output.status.code(), Some(130));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("human_handoff_failed") || stderr.contains("collaborative handoff failed"),
        "stderr={stderr}"
    );
    assert!(
        log.handoff_error_json.is_some(),
        "mock server must receive structured handoff_error"
    );
    assert!(
        log.handoff_error_json
            .as_deref()
            .is_some_and(|json| json.contains("human_handoff_failed")),
        "handoff_error.code must be human_handoff_failed"
    );
    assert!(
        log.held_connection_without_final_response,
        "server must hold agent-turn connection open without final response"
    );
    assert!(
        log.connection_closed_by_client,
        "server must observe client closing the agent-turn connection"
    );
    assert_eq!(
        log.mock_responses.len(),
        1,
        "server must send only the approval prompt, not a final response"
    );
}

#[test]
fn handoff_failure_abort_is_not_blocked_by_cancel_connection() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let started = Instant::now();
    let deadline = started + Duration::from_secs(8);
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::HoldConnectionOpen),
    );
    server.start_accept();

    let missing_aish = work.join("missing-aish");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify cancel connect does not block abort",
        ),
        deadline,
    );
    let log = server.join(deadline);
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "handoff failure abort must finish within bounded time even when cancel connect is not accepted (elapsed={:?})",
        started.elapsed()
    );
    assert!(
        !output.status.success(),
        "handoff failure must exit non-zero when cancel connect is not accepted"
    );
    assert_ne!(output.status.code(), Some(130));
    assert!(log.handoff_error_json.is_some());
    assert!(
        log.held_connection_without_final_response,
        "mock server must hold the agent_turn connection without accepting cancel"
    );
}

#[test]
fn handoff_failure_handles_server_disconnect() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::CloseConnection),
    );
    server.start_accept();

    let missing_aish = work.join("missing-aish");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify disconnect without final response",
        ),
        deadline,
    );
    let log = server.join(deadline);
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(130));
    assert!(log.handoff_error_json.is_some());
    assert!(
        !log.held_connection_without_final_response,
        "disconnect path must not use connection hold"
    );
}

#[test]
fn handoff_failure_server_receives_structured_error() {
    let ai_bin = PathBuf::from(env!("CARGO_BIN_EXE_ai"));
    let home = tempfile::tempdir().expect("home");
    let work = home.path().join("work");
    fs::create_dir_all(&work).expect("work");
    let deadline = Instant::now() + e2e_timeout();
    let mut server = CollaborativeMockServer::spawn(
        work.join("candidate"),
        ExpectedHandoffDecision::Failure(FailureFinalResponse::ToolError),
    );
    server.start_accept();
    let missing_aish = work.join("missing-aish-binary");
    let output = wait_child_with_timeout(
        spawn_ai_collaborative_failure(
            &ai_bin,
            home.path(),
            &work,
            &server.socket_path,
            &missing_aish,
            home.path(),
            "verify structured handoff error",
        ),
        deadline,
    );
    let log = server.join(deadline);
    assert!(!output.status.success());
    let error_json = log
        .handoff_error_json
        .expect("server must receive handoff_error");
    assert!(error_json.contains("human_handoff_failed"));
}

#[test]
#[should_panic(expected = "mock aibe server panicked")]
fn mock_server_panic_propagates() {
    let deadline = Instant::now() + Duration::from_secs(2);
    let handle = thread::spawn(|| panic!("mock aibe server panicked"));
    join_with_deadline(handle, deadline);
}

#[test]
fn e2e_has_bounded_timeout() {
    assert!(
        e2e_timeout() <= Duration::from_secs(60),
        "E2E timeout must be bounded"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/0055_collaborative_handoff_vertical_e2e.rs");
    let content = fs::read_to_string(&path).expect("read e2e source");
    let mut in_read_line_helper = false;
    let mut in_accept_helper = false;
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        if line.contains("fn read_line_with_deadline") {
            in_read_line_helper = true;
        }
        if line.contains("fn accept_with_deadline") {
            in_accept_helper = true;
        }
        if in_read_line_helper
            && line.starts_with("fn ")
            && !line.contains("read_line_with_deadline")
        {
            in_read_line_helper = false;
        }
        if in_accept_helper && line.starts_with("fn ") && !line.contains("accept_with_deadline") {
            in_accept_helper = false;
        }
        if line.contains("contains(") || line.trim_start().starts_with("assert!(") {
            continue;
        }
        assert!(
            !line.contains("let _ = handle.join()"),
            "unbounded join at line {line_no}: {line}"
        );
        if line.contains(".join()")
            && !line.contains("join_with_deadline")
            && !line.contains(".expect(")
        {
            panic!("unbounded join at line {line_no}: {line}");
        }
        if line.contains("listener.accept()") && !in_accept_helper {
            panic!("blocking accept at line {line_no}: {line}");
        }
        if line.contains(".read_line(")
            && !in_read_line_helper
            && !line.contains("read_line_with_deadline")
        {
            panic!("blocking read_line at line {line_no}: {line}");
        }
    }
}
