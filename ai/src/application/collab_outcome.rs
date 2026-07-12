use aibe_protocol::{
    CollabOutcome as WireOutcome, CollabOutcomeStatus as WireStatus, HumanHandoffResult,
};

use super::human_handoff::HumanHandoffExecutionResult;
use crate::domain::{CollabOutcome, CollabOutcomeStatus};

pub fn map_collab_handoff_result(
    handoff: HumanHandoffExecutionResult,
    outcome: CollabOutcome,
) -> HumanHandoffResult {
    HumanHandoffResult {
        collab_outcome: WireOutcome {
            status: match outcome.status {
                CollabOutcomeStatus::Done => WireStatus::Done,
                CollabOutcomeStatus::Blocked => WireStatus::Blocked,
                CollabOutcomeStatus::Cancelled => WireStatus::Cancelled,
            },
        },
        execution_outcome: handoff.execution_outcome,
        requested_command: handoff.requested_command,
        requested_command_completion: handoff.requested_command_completion,
        human_shell_exit_code: handoff.human_shell_exit_code,
        final_shell_cwd: handoff.final_shell_cwd,
        shell_log_range: handoff.shell_log_range,
        observation: handoff.observation,
    }
}
