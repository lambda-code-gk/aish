//! 0061 Collab Mode Human Task Evidence acceptance tests.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ai::adapters::outbound::{
    load_replay_events_in_range, ProcessEnvironmentObserver, MAX_EVIDENCE_SCAN_BYTES,
};
use ai::domain::human_task_evidence::{
    build_human_task_evidence, MAX_EVIDENCE_COMMANDS, MAX_EVIDENCE_COMMAND_BYTES,
    MAX_EVIDENCE_TOTAL_COMMAND_BYTES,
};
use ai::ports::outbound::EnvironmentObserver;
use aibe_protocol::{
    AgentTurnStatus, ClientRequest, ClientResponse, HandoffExecutionOutcome, HumanHandoffResult,
    HumanTaskEvidence, PostHandoffObservation, ProtocolMessageOut, RequestedCommandCompletion,
    ShellExecApprovalOrigin, ShellLogRange,
};
use aish_replay::{CommandKind, CommandSpec, LogEvent};
use serde_json::json;

fn shell_pair(index: u32, command: &str, exit_code: Option<i32>) -> [LogEvent; 2] {
    [
        LogEvent::shell_command_start(index, &format!("t{index}"), command),
        LogEvent::command_end(index, exit_code, &format!("f{index}")),
    ]
}

fn exec_pair(index: u32, command: &str) -> [LogEvent; 2] {
    let spec = CommandSpec {
        program: command.into(),
        args: vec![],
    };
    [
        LogEvent::command_start_span(&spec, index, &format!("t{index}"), CommandKind::Exec),
        LogEvent::command_end(index, Some(0), &format!("f{index}")),
    ]
}

fn write_events(dir: &tempfile::TempDir, events: &[LogEvent]) -> u64 {
    let contents = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(dir.path().join("log.jsonl"), &contents).unwrap();
    contents.len() as u64
}

fn observe(dir: &tempfile::TempDir, start: u64, end: Option<u64>) -> PostHandoffObservation {
    ProcessEnvironmentObserver::default().observe(dir.path(), start, end, Some(dir.path()))
}

fn standard_events() -> Vec<LogEvent> {
    let mut events = Vec::new();
    events.extend(shell_pair(0, "printf 'evidence-ok\\n'", Some(0)));
    events.extend(shell_pair(1, "false", Some(1)));
    events
}

fn regression_timeout() -> Duration {
    Duration::from_secs(15)
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
                    "mock server connection closed",
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
        "read_line deadline exceeded",
    ))
}

#[test]
fn human_task_evidence_is_collected_automatically() {
    let dir = tempfile::tempdir().unwrap();
    let end = write_events(&dir, &standard_events());
    let observation = observe(&dir, 0, Some(end));
    assert_eq!(observation.human_task_evidence.unwrap().commands.len(), 2);
}

#[test]
fn human_task_evidence_contains_commands_and_exit_codes() {
    let mut events = standard_events();
    events.extend(shell_pair(2, "unknown-exit", None));
    let evidence = build_human_task_evidence(&events, false).unwrap();
    assert_eq!(
        evidence
            .commands
            .iter()
            .map(|c| (&*c.command, c.exit_code))
            .collect::<Vec<_>>(),
        vec![
            ("printf 'evidence-ok\\n'", Some(0)),
            ("false", Some(1)),
            ("unknown-exit", None)
        ]
    );
}

#[test]
fn human_task_evidence_uses_handoff_log_range() {
    let dir = tempfile::tempdir().unwrap();
    let before = shell_pair(0, "before", Some(0));
    let inside = shell_pair(1, "inside", Some(0));
    let after = shell_pair(2, "after", Some(0));
    let before_text = before
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let inside_text = inside
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let after_text = after
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let contents = format!("{before_text}{inside_text}{after_text}");
    fs::write(dir.path().join("log.jsonl"), &contents).unwrap();
    let start = before_text.len() as u64;
    let end = start + inside_text.len() as u64;
    let loaded = load_replay_events_in_range(
        dir.path().join("log.jsonl").as_path(),
        start,
        Some(end),
        MAX_EVIDENCE_SCAN_BYTES,
    )
    .unwrap();
    let evidence = build_human_task_evidence(&loaded.events, loaded.truncated).unwrap();
    assert_eq!(evidence.commands.len(), 1);
    assert_eq!(evidence.commands[0].command, "inside");
}

#[test]
fn human_task_evidence_reuses_replay_spans() {
    let events = standard_events();
    let views = aish_replay::replay_span_views(&events).unwrap();
    let evidence = build_human_task_evidence(&events, false).unwrap();
    assert_eq!(evidence.commands.len(), views.len());
    assert_eq!(evidence.commands[0].command, views[0].command);
    assert_eq!(evidence.commands[1].exit_code, views[1].exit_code);
}

#[test]
fn human_task_evidence_excludes_non_shell_spans() {
    let mut events = Vec::new();
    events.extend(exec_pair(1, "tool"));
    events.push(LogEvent::shell_command_start(2, "t2", "incomplete"));
    events.extend(shell_pair(3, "kept", Some(0)));
    let evidence = build_human_task_evidence(&events, false).unwrap();
    assert_eq!(evidence.commands.len(), 1);
    assert_eq!(evidence.commands[0].command, "kept");
}

#[test]
fn human_task_evidence_is_bounded() {
    let mut events = Vec::new();
    for i in 0..(MAX_EVIDENCE_COMMANDS as u32 + 2) {
        events.extend(shell_pair(i, &format!("cmd-{i}"), Some(0)));
    }
    let evidence = build_human_task_evidence(&events, false).unwrap();
    assert_eq!(evidence.commands.len(), MAX_EVIDENCE_COMMANDS);
    assert!(evidence.truncated);
    let long = "あ".repeat(2000);
    let evidence_long = build_human_task_evidence(
        &[
            LogEvent::shell_command_start(0, "t0", &long),
            LogEvent::command_end(0, Some(0), "f0"),
        ],
        false,
    )
    .unwrap();
    assert!(evidence_long.commands[0].command.len() <= MAX_EVIDENCE_COMMAND_BYTES);
    assert!(evidence_long.truncated);
    assert!(MAX_EVIDENCE_TOTAL_COMMAND_BYTES >= MAX_EVIDENCE_COMMAND_BYTES);
    assert_eq!(MAX_EVIDENCE_SCAN_BYTES, 8 * 1024 * 1024);
}

#[test]
fn human_task_evidence_keeps_recent_commands() {
    let mut events = Vec::new();
    for i in 0..(MAX_EVIDENCE_COMMANDS as u32 + 3) {
        events.extend(shell_pair(i, &format!("cmd-{i}"), Some(0)));
    }
    let evidence = build_human_task_evidence(&events, false).unwrap();
    assert_eq!(evidence.commands[0].command, "cmd-3");
    assert_eq!(
        evidence.commands.last().unwrap().command,
        format!("cmd-{}", MAX_EVIDENCE_COMMANDS as u32 + 2)
    );
    assert!(evidence.truncated);
}

#[test]
fn human_task_evidence_preserves_redaction() {
    let events = [
        LogEvent::shell_command_start(
            0,
            "t0",
            "printf '%s\\n' 'APP_SECRET=collab-evidence-test-secret'",
        ),
        LogEvent::command_end(0, Some(0), "f0"),
    ];
    let evidence = build_human_task_evidence(&events, false).unwrap();
    let command = &evidence.commands[0].command;
    assert!(!command.contains("collab-evidence-test-secret"));
    assert!(command.contains("APP_SECRET=[REDACTED]"));
}

#[test]
fn human_task_evidence_distinguishes_empty_from_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("log.jsonl"), b"").unwrap();
    assert!(observe(&dir, 0, Some(0))
        .human_task_evidence
        .unwrap()
        .commands
        .is_empty());
    let end = write_events(&dir, &standard_events());
    assert!(!observe(&dir, 0, Some(end))
        .human_task_evidence
        .unwrap()
        .commands
        .is_empty());
    let unavailable = tempfile::tempdir().unwrap();
    assert!(observe(&unavailable, 0, None).human_task_evidence.is_none());
}

#[test]
fn human_task_evidence_failure_is_nonfatal() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("log.jsonl"), b"{not-json}\n").unwrap();
    let observation = observe(&dir, 0, Some(11));
    assert!(observation.human_task_evidence.is_none());
    assert!(observation.cwd_exists);
    assert!(observation
        .observation_errors
        .iter()
        .any(|e| e == "human_task_evidence_invalid_log"));
    let observation = observe(&dir, 3, Some(11));
    assert!(observation.human_task_evidence.is_none());
    assert!(observation
        .observation_errors
        .iter()
        .any(|e| e == "human_task_evidence_invalid_range"));
    assert_eq!(
        observation
            .observation_errors
            .iter()
            .filter(|e| e.as_str() == "human_task_evidence_invalid_range")
            .count(),
        1
    );
}

#[test]
fn human_task_evidence_requires_no_manual_summary() {
    let dir = tempfile::tempdir().unwrap();
    let end = write_events(&dir, &standard_events());
    assert!(observe(&dir, 0, Some(end)).human_task_evidence.is_some());
}

#[test]
fn human_task_evidence_does_not_infer_completion() {
    let dir = tempfile::tempdir().unwrap();
    let end = write_events(&dir, &standard_events());
    let result = HumanHandoffResult {
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        requested_command: Some("candidate".into()),
        requested_command_completion: RequestedCommandCompletion::Unknown,
        human_shell_exit_code: Some(0),
        final_shell_cwd: Some(dir.path().display().to_string()),
        shell_log_range: Some(ShellLogRange {
            start: 0,
            end: Some(end),
        }),
        observation: Some(observe(&dir, 0, Some(end))),
    };
    let encoded = serde_json::to_value(&result).unwrap();
    assert_eq!(encoded["requested_command_completion"], "unknown");
    assert_eq!(
        encoded["observation"]["human_task_evidence"]["commands"][1]["exit_code"],
        1
    );
}

#[test]
fn human_task_evidence_protocol_is_backward_compatible() {
    let old = json!({"cwd_exists": true, "cwd": "/tmp", "observation_errors": []});
    let decoded: PostHandoffObservation = serde_json::from_value(old).unwrap();
    assert!(decoded.human_task_evidence.is_none());
    let encoded = serde_json::to_value(decoded).unwrap();
    assert!(encoded.get("human_task_evidence").is_none());
    let empty: HumanTaskEvidence = serde_json::from_value(json!({})).unwrap();
    assert!(empty.commands.is_empty());
    assert!(!empty.truncated);
}

#[test]
fn human_task_evidence_normal_mode_regression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("aibe.sock");
    let _ = fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("bind");
    let saw_forbidden = Arc::new(Mutex::new(None));
    let saw_forbidden_thread = Arc::clone(&saw_forbidden);
    let deadline = Instant::now() + regression_timeout();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking");
        let (stream, _) = loop {
            if Instant::now() >= deadline {
                panic!("accept timed out");
            }
            match listener.accept() {
                Ok(conn) => break conn,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("accept: {e}"),
            }
        };
        let mut writer = stream.try_clone().expect("clone");
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        read_line_with_deadline(&mut reader, &mut line, deadline).expect("read turn");
        let req: ClientRequest = serde_json::from_str(line.trim()).expect("parse");
        let ClientRequest::AgentTurn { id, context, .. } = req else {
            panic!("expected agent_turn");
        };
        assert!(
            !context.collaborative_handoff,
            "non-collaborative request must not set collaborative_handoff"
        );

        let prompt = ClientResponse::ShellExecApprovalPrompt {
            id: "prompt-1".into(),
            turn_id: id.clone(),
            tool_call_id: "call-1".into(),
            command: "echo".into(),
            args: vec!["ok".into()],
        };
        writeln!(writer, "{}", serde_json::to_string(&prompt).unwrap()).unwrap();

        line.clear();
        read_line_with_deadline(&mut reader, &mut line, deadline).expect("read approval");
        let approval: ClientRequest = serde_json::from_str(line.trim()).expect("approval");
        let ClientRequest::ShellExecApproval {
            approval_origin,
            handoff_result,
            handoff_error,
            ..
        } = approval
        else {
            panic!("expected approval");
        };
        if approval_origin == ShellExecApprovalOrigin::CollaborativeHandoff
            || handoff_result.is_some()
            || handoff_error.is_some()
        {
            *saw_forbidden_thread.lock().unwrap() = Some(format!(
                "origin={approval_origin:?} handoff_result={} handoff_error={}",
                handoff_result.is_some(),
                handoff_error.is_some()
            ));
        }

        let result = ClientResponse::AgentTurnResult {
            id,
            status: AgentTurnStatus::Ok,
            assistant_message: ProtocolMessageOut {
                role: "assistant".into(),
                content: "echo ok".into(),
            },
            tool_calls: vec![],
        };
        writeln!(writer, "{}", serde_json::to_string(&result).unwrap()).unwrap();
    });

    let home = tempfile::tempdir().expect("home");
    let history_dir = home.path().join("history");
    fs::create_dir_all(&history_dir).unwrap();
    let ai_cfg = home.path().join("ai.toml");
    fs::write(
        &ai_cfg,
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
    .unwrap();
    let aibe_cfg = home.path().join("aibe.toml");
    fs::write(
        &aibe_cfg,
        r#"
[tools.shell_exec]
shell_exec_approval = "ask"
"#,
    )
    .unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_ai"));
    command
        .env("AI_CONFIG", &ai_cfg)
        .env("AIBE_CONFIG", &aibe_cfg)
        .env("HOME", home.path())
        .env(
            "AISH_BIN",
            Path::new(env!("CARGO_BIN_EXE_ai"))
                .parent()
                .unwrap()
                .join("aish"),
        )
        .args(["ask", "--quiet", "--no-start", "run echo"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = spawn_in_new_process_group(&mut command);
    let output = wait_child_with_timeout(child, deadline);
    handle.join().expect("join mock");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        *saw_forbidden.lock().unwrap(),
        None,
        "non-collaborative shell_exec must not carry handoff Evidence"
    );

    let aish = Path::new(env!("CARGO_BIN_EXE_ai"))
        .parent()
        .unwrap()
        .join("aish");
    let aish_out = Command::new(&aish)
        .args(["exec", "--", "true"])
        .env("HOME", home.path())
        .env("SHELL", "/bin/bash")
        .output()
        .expect("aish exec");
    assert!(aish_out.status.success());
    let stderr = String::from_utf8_lossy(&aish_out.stderr);
    assert!(!stderr.contains("AISH Collaborative Mode"));
    assert!(!stderr.contains("human_task_evidence"));
}

#[test]
fn human_task_evidence_vertical_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let end = write_events(&dir, &standard_events());
    let result = HumanHandoffResult {
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        requested_command: Some("candidate".into()),
        requested_command_completion: RequestedCommandCompletion::Unknown,
        human_shell_exit_code: Some(0),
        final_shell_cwd: Some(dir.path().display().to_string()),
        shell_log_range: Some(ShellLogRange {
            start: 0,
            end: Some(end),
        }),
        observation: Some(observe(&dir, 0, Some(end))),
    };
    let wire = serde_json::to_value(result).unwrap();
    let commands = wire["observation"]["human_task_evidence"]["commands"]
        .as_array()
        .unwrap();
    assert_eq!(commands[0]["exit_code"], 0);
    assert_eq!(commands[1]["command"], "false");
    assert_eq!(commands[1]["exit_code"], 1);
    assert_eq!(wire["requested_command_completion"], "unknown");
}

fn spawn_in_new_process_group(command: &mut Command) -> Child {
    use std::os::unix::process::CommandExt;
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command.spawn().expect("spawn child")
}

fn wait_child_with_timeout(mut child: Child, deadline: Instant) -> std::process::Output {
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().expect("wait output"),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let pgid = child.id() as i32;
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
                return child.wait_with_output().expect("wait after kill");
            }
            Err(e) => panic!("wait child failed: {e}"),
        }
    }
}
