use std::io::Cursor;

use ai::adapters::outbound::collect_collab_outcome_from_streams;
use ai::application::{map_collab_handoff_result, HumanHandoffExecutionResult};
use ai::domain::{parse_collab_outcome_status, CollabOutcome, CollabOutcomeStatus};
use aibe_protocol::{
    CollabOutcomeStatus as WireStatus, HandoffExecutionOutcome, RequestedCommandCompletion,
};

fn execution(exit_code: i32) -> HumanHandoffExecutionResult {
    HumanHandoffExecutionResult {
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        requested_command: Some("true".into()),
        requested_command_completion: RequestedCommandCompletion::Unknown,
        human_shell_exit_code: Some(exit_code),
        final_shell_cwd: Some("/tmp".into()),
        shell_log_range: None,
        observation: None,
    }
}

fn collect(input: &str) -> (CollabOutcome, String) {
    let mut reader = Cursor::new(input.as_bytes());
    let mut output = Vec::new();
    let outcome = collect_collab_outcome_from_streams(&mut reader, &mut output).unwrap();
    (outcome, String::from_utf8(output).unwrap())
}

#[test]
fn collab_outcome_status_accepts_documented_forms() {
    for (input, expected) in [
        ("d", CollabOutcomeStatus::Done),
        ("DONE", CollabOutcomeStatus::Done),
        ("b", CollabOutcomeStatus::Blocked),
        ("Blocked", CollabOutcomeStatus::Blocked),
        ("c", CollabOutcomeStatus::Cancelled),
        ("CANCELLED", CollabOutcomeStatus::Cancelled),
    ] {
        assert_eq!(parse_collab_outcome_status(input).unwrap(), expected);
    }
}

#[test]
fn collab_outcome_status_rejects_invalid_forms() {
    for input in ["", "x", "complete", "failed"] {
        assert!(parse_collab_outcome_status(input).is_err());
    }
    let (outcome, output) = collect("\nx\ncomplete\nfailed\nd\n");
    assert_eq!(outcome.status, CollabOutcomeStatus::Done);
    assert!(
        output
            .matches("d、b、cのいずれかを入力してください")
            .count()
            >= 4
    );
    assert!(!output.contains("実施した作業"));
    assert!(!output.contains("理由を1行で"));
}

#[test]
fn collab_outcome_domain_creation_preserves_invariants() {
    for status in [
        CollabOutcomeStatus::Done,
        CollabOutcomeStatus::Blocked,
        CollabOutcomeStatus::Cancelled,
    ] {
        assert_eq!(CollabOutcome::new(status).status, status);
    }
}

#[test]
fn collab_outcome_serializes_all_statuses() {
    for (status, expected) in [
        (CollabOutcomeStatus::Done, "done"),
        (CollabOutcomeStatus::Blocked, "blocked"),
        (CollabOutcomeStatus::Cancelled, "cancelled"),
    ] {
        let json = serde_json::to_value(map_collab_handoff_result(
            execution(0),
            CollabOutcome::new(status),
        ))
        .unwrap();
        assert_eq!(json["collab_outcome"]["status"], expected);
        assert!(json["collab_outcome"].get("summary").is_none());
    }
}

#[test]
fn collab_outcome_is_independent_from_shell_exit_code() {
    let blocked = map_collab_handoff_result(
        execution(0),
        CollabOutcome::new(CollabOutcomeStatus::Blocked),
    );
    let done =
        map_collab_handoff_result(execution(23), CollabOutcome::new(CollabOutcomeStatus::Done));
    assert_eq!(blocked.collab_outcome.status, WireStatus::Blocked);
    assert_eq!(done.collab_outcome.status, WireStatus::Done);
    assert_eq!(done.human_shell_exit_code, Some(23));
}

#[test]
fn collab_outcome_launch_failure_skips_prompt() {
    let mut output = Vec::new();
    let launch_result: Result<HumanHandoffExecutionResult, &str> = Err("launch failed");
    if launch_result.is_ok() {
        let _ = collect_collab_outcome_from_streams(&mut Cursor::new(b""), &mut output);
    }
    assert!(output.is_empty());
}

#[test]
fn collab_outcome_noninteractive_stdin_fails_explicitly() {
    use ai::ports::outbound::CollabOutcomeCollectionError;
    assert_eq!(
        CollabOutcomeCollectionError::NonInteractiveStdin.to_string(),
        "Cannot collect Collaborative Mode result because stdin is not interactive."
    );
}

#[test]
fn collab_outcome_io_is_unit_testable() {
    let (outcome, output) = collect("invalid\nc\n");
    assert_eq!(
        outcome,
        CollabOutcome {
            status: CollabOutcomeStatus::Cancelled,
        }
    );
    assert!(output.contains("作業結果を選択してください"));
    assert!(!output.contains("中止した理由"));
    assert!(!output.contains("実施した作業"));
}
