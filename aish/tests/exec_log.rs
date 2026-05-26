use std::fs;

use aish::adapters::outbound::{JsonlFileLog, ProcessShell};
use aish::application::ExecuteAndRecord;
use aish::domain::CommandSpec;

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
    assert!(content.contains(r#""event":"exit""#));
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
