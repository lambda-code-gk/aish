#![cfg(unix)]

use std::fs;
use std::process::Command;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("observation.jsonl");
    fs::write(
        &path,
        concat!(
            "{\"timestamp_ms\":100,\"ai_session_id\":\"s1\",\"mode\":\"assist\",\"intent\":\"debug\",\"gate\":\"assist\",\"decision_path\":\"route_turn\",\"route_turn_used\":true,\"local_route_used\":true,\"total_turn_latency_ms\":10,\"estimated_tokens_saved\":3,\"context_needs\":[\"git_diff\"],\"tool_hints\":[\"shell\"],\"reason_codes\":[\"debug_signal\"],\"raw_user_text\":\"TOP SECRET\"}\n",
            "broken-json\n",
            "{\"timestamp_ms\":200,\"ai_session_id\":\"s2\",\"mode\":\"gate\",\"intent\":\"simple_chat\",\"gate\":\"short_circuit\",\"decision_path\":\"local\",\"short_circuit_allowed\":true,\"total_turn_latency_ms\":20}\n"
        ),
    )
    .expect("write fixture");
    (dir, path)
}

fn ai(path: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai"));
    command.args(["smart"]);
    command.args(args);
    command.args(["--path", path.to_str().expect("utf8 path")]);
    command.output().expect("run ai")
}

#[test]
fn smart_stats_json_cli_reports_invalid_lines() {
    let (_dir, path) = fixture();
    let output = ai(&path, &["stats", "--format", "json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["total_records"], 3);
    assert_eq!(value["valid_records"], 2);
    assert_eq!(value["invalid_lines"], 1);
    assert_eq!(value["by_mode"]["assist"], 1);
}

#[test]
fn smart_stats_tsv_cli_uses_key_value_rows() {
    let (_dir, path) = fixture();
    let output = ai(&path, &["stats", "--format", "tsv"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("total_records\t3\n"));
    assert!(stdout.contains("latency.total_turn.p50_ms\t10\n"));
}

#[test]
fn smart_recent_json_cli_outputs_only_known_safe_fields() {
    let (_dir, path) = fixture();
    let output = ai(&path, &["recent", "--format", "json", "--limit", "2"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(value.as_array().expect("array").len(), 1);
    assert!(!stdout.contains("TOP SECRET"));
    assert!(!stdout.contains("raw_user_text"));
}

#[test]
fn smart_report_cli_outputs_markdown_without_raw_user_text() {
    let (_dir, path) = fixture();
    let output = ai(&path, &["report", "--include-recent", "2"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.starts_with("# AISH Smart Preprocessor Observation Report"));
    assert!(stdout.contains("## Notes for AI Evaluation"));
    assert!(!stdout.contains("TOP SECRET"));
    assert!(!stdout.contains("raw_user_text"));
}
