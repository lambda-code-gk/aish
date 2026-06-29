#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread::JoinHandle;

use aibe_protocol::{
    ClientRequest, ClientResponse, ErrorCode, WorkApplyRequestBody, WorkApplyResponseBody,
    WorkEntryDto, WorkEntryKindDto, WorkItemDto, WorkMutationKindDto, WorkMutationOutcomeDto,
    WorkOperationDto, WorkQueryResponseBody, WorkSnapshotDto, WorkStatusDto,
};

struct WorkServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
}

impl WorkServer {
    fn empty_queries(expected: usize) -> Self {
        Self::queries(vec![WorkSnapshotDto::default(); expected])
    }

    fn queries(snapshots: Vec<WorkSnapshotDto>) -> Self {
        let dir = tempfile::tempdir().expect("socket dir");
        let socket_path = dir.path().join("aibe.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = std::thread::spawn(move || {
            for snapshot in snapshots {
                let (stream, _) = listener.accept().expect("accept");
                let mut writer = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request");
                let request: ClientRequest =
                    serde_json::from_str(line.trim()).expect("parse request");
                let ClientRequest::WorkQuery(body) = request else {
                    panic!("expected work_query: {request:?}");
                };
                let response = ClientResponse::WorkQueryResult(WorkQueryResponseBody {
                    id: body.id,
                    snapshot,
                });
                writeln!(
                    writer,
                    "{}",
                    serde_json::to_string(&response).expect("serialize response")
                )
                .expect("write response");
            }
        });
        Self {
            handle: Some(handle),
            _dir: dir,
            socket_path,
        }
    }

    fn apply(
        handler: impl FnOnce(WorkApplyRequestBody) -> ClientResponse + Send + 'static,
    ) -> Self {
        let dir = tempfile::tempdir().expect("socket dir");
        let socket_path = dir.path().join("aibe.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut writer = stream.try_clone().expect("clone");
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read request");
            let request: ClientRequest = serde_json::from_str(line.trim()).expect("parse request");
            let ClientRequest::WorkApply(body) = request else {
                panic!("expected work_apply: {request:?}");
            };
            writeln!(
                writer,
                "{}",
                serde_json::to_string(&handler(body)).expect("serialize response")
            )
            .expect("write response");
        });
        Self {
            handle: Some(handle),
            _dir: dir,
            socket_path,
        }
    }
}

impl Drop for WorkServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.join().expect("server join");
        }
    }
}

#[test]
fn work_dashboard_status_and_list_render_empty_state() {
    let server = WorkServer::empty_queries(3);
    let home = tempfile::tempdir().expect("home");
    let config = home.path().join("ai.toml");
    fs::write(
        &config,
        format!(
            "socket_path = {:?}\n",
            server.socket_path.display().to_string()
        ),
    )
    .expect("write config");

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ai"))
            .env("HOME", home.path())
            .env("AI_CONFIG", &config)
            .env("AI_SESSION_ID", "work-phase-zero")
            .args(args)
            .output()
            .expect("run ai work")
    };

    let dashboard = run(&["work", "--no-start"]);
    assert!(dashboard.status.success());
    let dashboard = String::from_utf8_lossy(&dashboard.stdout);
    assert!(dashboard.contains("No active work."));
    assert!(dashboard.contains("Start a new work:"));

    let status = run(&["work", "status", "--no-start"]);
    assert!(status.status.success());
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(status.contains("No active work."));
    assert!(status.contains("Start one:"));
    assert!(!status.contains("Useful commands:"));

    let list = run(&["work", "list", "--no-start"]);
    assert!(list.status.success());
    let list = String::from_utf8_lossy(&list.stdout);
    for section in ["Active:", "Paused:", "Deferred:", "Done:"] {
        assert!(list.contains(section), "missing {section}: {list}");
    }
}

#[test]
fn work_status_renders_all_required_sections() {
    let snapshot = populated_snapshot();
    let server = WorkServer::queries(vec![snapshot.clone(), snapshot]);
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);

    for args in [
        vec!["work", "--no-start"],
        vec!["work", "status", "--no-start"],
    ] {
        let output = run_work(&home, &config, &args);
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut previous = 0;
        for section in [
            "Active work:",
            "Focus:",
            "Stack:",
            "Decisions:",
            "Ideas:",
            "Deferred:",
            "Suggested next:",
        ] {
            let position = stdout.find(section).expect("required section");
            assert!(position >= previous, "section order: {stdout}");
            previous = position;
        }
        for content in [
            "active goal",
            "current focus",
            "paused work",
            "use one store",
            "try dashboard",
            "later task",
        ] {
            assert!(stdout.contains(content), "missing {content}: {stdout}");
        }
    }
}

#[test]
fn work_list_groups_works_by_status() {
    let server = WorkServer::queries(vec![populated_snapshot()]);
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);
    let output = run_work(&home, &config, &["work", "list", "--no-start"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Active:\n  #2 active goal",
        "Paused:\n  #1 paused work [stack]",
        "Deferred:\n  #3 later task",
        "Done:\n  #4 done work",
    ] {
        assert!(stdout.contains(expected), "missing {expected}: {stdout}");
    }
}

#[test]
fn work_start_uses_apply_rpc_and_renders_human_output() {
    let server = WorkServer::apply(|body| {
        assert_eq!(
            body.operation,
            WorkOperationDto::Start {
                goal: "new goal".into()
            }
        );
        ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            id: body.id,
            snapshot: WorkSnapshotDto {
                revision: 1,
                active_work_id: Some(1),
                stack: Vec::new(),
                works: vec![work(1, "new goal", WorkStatusDto::Active, None)],
                entries: Vec::new(),
            },
            outcome: WorkMutationOutcomeDto {
                kind: WorkMutationKindDto::Start,
                work_id: Some(1),
                previous_work_id: None,
            },
        })
    });
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);
    let output = run_work(
        &home,
        &config,
        &["work", "start", "--no-start", "new", "goal"],
    );
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Started work #1:\n  new goal\n\nActive work is now #1.\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn work_phase1_commands_use_apply_rpc_and_render_human_output() {
    let cases = [
        (
            vec!["work", "focus", "--no-start", "next", "step"],
            WorkOperationDto::Focus {
                text: "next step".into(),
            },
            "Updated focus for work #1:\n  next step\n",
        ),
        (
            vec!["work", "idea", "--no-start", "try", "this"],
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Idea,
                text: "try this".into(),
            },
            "Added idea to work #1:\n  try this\n",
        ),
        (
            vec!["work", "note", "--no-start", "observed", "this"],
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Note,
                text: "observed this".into(),
            },
            "Added note to work #1:\n  observed this\n",
        ),
        (
            vec!["work", "decide", "--no-start", "keep", "this"],
            WorkOperationDto::AddEntry {
                kind: WorkEntryKindDto::Decision,
                text: "keep this".into(),
            },
            "Added decision to work #1:\n  keep this\n",
        ),
        (
            vec!["work", "defer", "--no-start", "later", "task"],
            WorkOperationDto::Defer {
                text: "later task".into(),
            },
            "Deferred work #1:\n  later task\n",
        ),
    ];

    for (args, expected_operation, expected_stdout) in cases {
        let response_operation = expected_operation.clone();
        let server = WorkServer::apply(move |body| {
            assert_eq!(body.operation, expected_operation);
            phase1_success_response(body.id, response_operation)
        });
        let home = tempfile::tempdir().expect("home");
        let config = write_config(&home, &server.socket_path);
        let output = run_work(&home, &config, &args);
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected_stdout);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn work_apply_protocol_error_is_stderr_and_nonzero() {
    let server = WorkServer::apply(|body| {
        ClientResponse::error(body.id, ErrorCode::InvalidRequest, "no active work")
    });
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);
    let output = run_work(&home, &config, &["work", "focus", "--no-start", "next"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no active work"));
}

#[test]
fn work_apply_unexpected_response_does_not_dump_snapshot() {
    let server = WorkServer::apply(|body| {
        ClientResponse::WorkQueryResult(WorkQueryResponseBody {
            id: body.id,
            snapshot: WorkSnapshotDto {
                revision: 1,
                active_work_id: Some(1),
                stack: Vec::new(),
                works: vec![work(1, "secret-work-content", WorkStatusDto::Active, None)],
                entries: Vec::new(),
            },
        })
    });
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);
    let output = run_work(&home, &config, &["work", "focus", "--no-start", "next"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected work response"));
    assert!(!stderr.contains("secret-work-content"));
}

#[test]
fn work_apply_inconsistent_success_is_rejected_without_dumping_snapshot() {
    let server = WorkServer::apply(|body| {
        ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            id: body.id,
            snapshot: WorkSnapshotDto {
                revision: 1,
                active_work_id: Some(1),
                stack: Vec::new(),
                works: vec![work(1, "secret-work-content", WorkStatusDto::Active, None)],
                entries: Vec::new(),
            },
            outcome: WorkMutationOutcomeDto {
                kind: WorkMutationKindDto::Focus,
                work_id: Some(1),
                previous_work_id: None,
            },
        })
    });
    let home = tempfile::tempdir().expect("home");
    let config = write_config(&home, &server.socket_path);
    let output = run_work(&home, &config, &["work", "focus", "--no-start", "next"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid work response"));
    assert!(!stderr.contains("secret-work-content"));
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_views_render_stack_and_child_marker() {
    panic!("pending 0052");
}

fn write_config(home: &tempfile::TempDir, socket_path: &std::path::Path) -> std::path::PathBuf {
    let config = home.path().join("ai.toml");
    fs::write(
        &config,
        format!("socket_path = {:?}\n", socket_path.display().to_string()),
    )
    .expect("write config");
    config
}

fn run_work(
    home: &tempfile::TempDir,
    config: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ai"))
        .env("HOME", home.path())
        .env("AI_CONFIG", config)
        .env("AI_SESSION_ID", "work-phase-one")
        .args(args)
        .output()
        .expect("run ai work")
}

fn populated_snapshot() -> WorkSnapshotDto {
    WorkSnapshotDto {
        revision: 8,
        active_work_id: Some(2),
        stack: vec![1],
        works: vec![
            work(1, "paused work", WorkStatusDto::Paused, None),
            WorkItemDto {
                parent_id: Some(1),
                ..work(
                    2,
                    "active goal",
                    WorkStatusDto::Active,
                    Some("current focus"),
                )
            },
            work(3, "later task", WorkStatusDto::Deferred, None),
            work(4, "done work", WorkStatusDto::Done, None),
        ],
        entries: vec![
            WorkEntryDto {
                id: 1,
                work_id: 2,
                kind: WorkEntryKindDto::Decision,
                text: "use one store".into(),
                created_at_ms: 10,
            },
            WorkEntryDto {
                id: 2,
                work_id: 2,
                kind: WorkEntryKindDto::Idea,
                text: "try dashboard".into(),
                created_at_ms: 11,
            },
        ],
    }
}

fn phase1_success_response(id: String, operation: WorkOperationDto) -> ClientResponse {
    let (active_work_id, status, focus, entries, kind) = match &operation {
        WorkOperationDto::Focus { text } => (
            Some(1),
            WorkStatusDto::Active,
            Some(text.as_str()),
            Vec::new(),
            WorkMutationKindDto::Focus,
        ),
        WorkOperationDto::AddEntry { kind, text } => (
            Some(1),
            WorkStatusDto::Active,
            None,
            vec![WorkEntryDto {
                id: 1,
                work_id: 1,
                kind: *kind,
                text: text.clone(),
                created_at_ms: 1,
            }],
            WorkMutationKindDto::AddEntry,
        ),
        WorkOperationDto::Defer { .. } => (
            None,
            WorkStatusDto::Deferred,
            None,
            Vec::new(),
            WorkMutationKindDto::Defer,
        ),
        other => panic!("unsupported phase 1 test operation: {other:?}"),
    };
    let title = match &operation {
        WorkOperationDto::Defer { text } => text.as_str(),
        _ => "active goal",
    };
    ClientResponse::WorkApplyResult(WorkApplyResponseBody {
        id,
        snapshot: WorkSnapshotDto {
            revision: 2,
            active_work_id,
            stack: Vec::new(),
            works: vec![work(1, title, status, focus)],
            entries,
        },
        outcome: WorkMutationOutcomeDto {
            kind,
            work_id: Some(1),
            previous_work_id: None,
        },
    })
}

fn work(id: u64, title: &str, status: WorkStatusDto, focus: Option<&str>) -> WorkItemDto {
    WorkItemDto {
        id,
        title: title.into(),
        goal: title.into(),
        status,
        parent_id: None,
        created_at_ms: id,
        updated_at_ms: id,
        finished_at_ms: (status == WorkStatusDto::Done).then_some(id),
        focus: focus.map(str::to_string),
        summary: None,
    }
}
