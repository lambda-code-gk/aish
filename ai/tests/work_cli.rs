#![cfg(unix)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread::JoinHandle;

use aibe_protocol::{ClientRequest, ClientResponse, WorkQueryResponseBody, WorkSnapshotDto};

struct WorkServer {
    handle: Option<JoinHandle<()>>,
    _dir: tempfile::TempDir,
    socket_path: std::path::PathBuf,
}

impl WorkServer {
    fn empty_queries(expected: usize) -> Self {
        let dir = tempfile::tempdir().expect("socket dir");
        let socket_path = dir.path().join("aibe.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind");
        let handle = std::thread::spawn(move || {
            for _ in 0..expected {
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
                    snapshot: WorkSnapshotDto::default(),
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
#[ignore = "0052 phase 1 pending"]
fn work_status_renders_all_required_sections() {
    panic!("pending 0052");
}

#[test]
#[ignore = "0052 phase 1 pending"]
fn work_list_groups_works_by_status() {
    panic!("pending 0052");
}

#[test]
#[ignore = "0052 phase 3 pending"]
fn work_views_render_stack_and_child_marker() {
    panic!("pending 0052");
}
