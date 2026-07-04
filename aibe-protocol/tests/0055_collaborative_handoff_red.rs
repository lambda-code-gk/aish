// 0055 Collaborative Human Handoff protocol tests.

use aibe_protocol::{
    HandoffExecutionOutcome, HumanHandoffResult, RequestedCommandCompletion, ShellLogRange,
    UncertainToolExecution,
};

#[test]
fn human_handoff_result_serde_roundtrip() {
    let value = HumanHandoffResult {
        handoff_id: "ho-1".to_string(),
        execution_outcome: HandoffExecutionOutcome::HumanControlReturned,
        return_reason: Some("ctrl_d".to_string()),
        human_shell_exit_code: Some(0),
        requested_command: Some("cargo test".to_string()),
        requested_command_completion: RequestedCommandCompletion::Unknown,
        final_shell_cwd: Some("/tmp/work".to_string()),
        shell_log_range: Some(ShellLogRange {
            start: 10,
            end: Some(20),
        }),
        child_goal_summary: Some("child".to_string()),
        side_conversation_summary: None,
        before_observation_ref: Some("obs-before".to_string()),
        after_observation_ref: Some("obs-after".to_string()),
        uncertain_tool_executions: vec![UncertainToolExecution {
            tool_call_id: "tc-1".to_string(),
            tool_name: "shell_exec".to_string(),
            status: "unknown".to_string(),
        }],
    };
    let json = serde_json::to_string(&value).expect("serialize");
    let decoded: HumanHandoffResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, value);
}
