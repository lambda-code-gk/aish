use std::fs;

use aish::adapters::outbound::{
    read_log_events, resolve_replay_log_path, JsonlFileLog, ProcessShell,
};
use aish::application::{replay_list, replay_show, ExecuteAndRecord};
use aish::domain::{CommandSpec, LogEvent, OutputFormat};

#[test]
fn exec_echo_writes_jsonl_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("test.jsonl");

    let log = JsonlFileLog::new(log_path.clone());
    let mut app = ExecuteAndRecord::new(ProcessShell, log);
    app.run(CommandSpec {
        program: "echo".to_string(),
        args: vec!["hello".to_string()],
    })
    .expect("run");

    let content = fs::read_to_string(&log_path).expect("read log");
    assert!(content.contains(r#""event":"command_start""#));
    assert!(content.contains(r#""event":"stdout""#));
    assert!(content.contains("hello"));
    assert!(content.contains(r#""event":"command_end""#));
}

#[test]
fn exec_command_span_records_index_and_timestamps() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("span.jsonl");

    let log = JsonlFileLog::new(log_path.clone());
    let mut app = ExecuteAndRecord::new(ProcessShell, log);
    app.run(CommandSpec {
        program: "echo".to_string(),
        args: vec!["hello".to_string()],
    })
    .expect("run");

    let content = fs::read_to_string(&log_path).expect("read log");
    assert!(content.contains(r#""command_index":1"#));
    assert!(content.contains(r#""kind":"exec""#));
    assert!(content.contains(r#""started_at""#));
    assert!(content.contains(r#""finished_at""#));
}

#[test]
fn exec_masks_secrets_in_command_start() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("secrets.jsonl");
    let secret = "sk-abcdefghijklmnopqrst";

    let log = JsonlFileLog::new(log_path.clone());
    let mut app = ExecuteAndRecord::new(ProcessShell, log);
    app.run(CommandSpec {
        program: "echo".to_string(),
        args: vec![secret.to_string()],
    })
    .expect("run");

    let content = fs::read_to_string(&log_path).expect("read log");
    assert!(content.contains(r#""event":"command_start""#));
    assert!(!content.contains(secret));
    assert!(content.contains("sk-[REDACTED]"));
}

#[test]
fn replay_cli_list_and_show_with_explicit_log() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("replay.jsonl");
    let log = JsonlFileLog::new(log_path.clone());
    let mut app = ExecuteAndRecord::new(ProcessShell, log);
    app.run(CommandSpec {
        program: "echo".to_string(),
        args: vec!["replay-me".to_string()],
    })
    .expect("run");

    let events = read_log_events(&log_path).expect("read");
    let list = replay_list(&events, None, OutputFormat::Tsv).expect("list");
    assert!(list.contains("replay-me"));

    let out = replay_show(&events, 1, false).expect("show");
    assert!(out.contains("replay-me"));
}

#[test]
fn replay_show_rejects_shell_stderr_via_application() {
    let events = vec![
        LogEvent::shell_command_start(1, "2026-01-01T00:00:00Z", "echo hi"),
        LogEvent::stdout_indexed("hi\n", 1),
        LogEvent::command_end(1, Some(0), "2026-01-01T00:00:01Z"),
    ];
    let err = replay_show(&events, 1, true).expect_err("stderr");
    assert!(matches!(
        err,
        aish::application::ReplayError::ShellStderrNotSupported
    ));
}

#[test]
fn replay_current_log_resolution_rejects_escape() {
    let root = tempfile::tempdir().expect("tempdir");
    let session = root.path().join("002f15d02b54");
    fs::create_dir(&session).expect("mkdir");
    fs::write(root.path().join("outside.jsonl"), "x").expect("write outside");
    std::os::unix::fs::symlink("../outside.jsonl", session.join("current_log")).expect("link");

    std::env::set_var("AISH_SESSION_DIR", &session);
    let err = resolve_replay_log_path(None).expect_err("escape");
    assert!(matches!(
        err,
        aish::adapters::outbound::ReplayLogResolveError::SymlinkEscape
    ));
    std::env::remove_var("AISH_SESSION_DIR");
}
