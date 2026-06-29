//! Work CLI handler。

use aibe_protocol::{
    ClientResponse, WorkApplyResponseBody, WorkItemDto, WorkMutationKindDto, WorkOperationDto,
    WorkQueryResponseBody, WorkSnapshotDto, WorkStatusDto,
};

use super::api::{AgentError, MemoryCliContext};
use crate::domain::{render_work_snapshot, WorkView};
use crate::ports::outbound::WorkClient;

pub fn run_work_query(
    client: &dyn WorkClient,
    ctx: &MemoryCliContext,
    view: WorkView,
) -> Result<String, AgentError> {
    match client.work_query(&ctx.session_id, &ctx.memory_context)? {
        ClientResponse::WorkQueryResult(WorkQueryResponseBody { snapshot, .. }) => {
            Ok(render_work_snapshot(&snapshot, view))
        }
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        _ => Err(AgentError::Request("unexpected work response".into())),
    }
}

pub fn run_work_apply(
    client: &dyn WorkClient,
    ctx: &MemoryCliContext,
    operation: WorkOperationDto,
) -> Result<String, AgentError> {
    match client.work_apply(&ctx.session_id, &ctx.memory_context, operation.clone())? {
        ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            snapshot, outcome, ..
        }) => render_apply_result(&operation, &snapshot, &outcome),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        _ => Err(AgentError::Request("unexpected work response".into())),
    }
}

fn render_apply_result(
    operation: &WorkOperationDto,
    snapshot: &WorkSnapshotDto,
    outcome: &aibe_protocol::WorkMutationOutcomeDto,
) -> Result<String, AgentError> {
    if outcome.kind != mutation_kind(operation) {
        return Err(invalid_response());
    }
    let work_id = outcome.work_id.ok_or_else(invalid_response)?;
    let work = snapshot
        .works
        .iter()
        .find(|work| work.id == work_id)
        .ok_or_else(invalid_response)?;
    validate_phase1_result(operation, snapshot, outcome, work)?;
    let rendered = match operation {
        WorkOperationDto::Start { goal } => {
            let paused = outcome
                .previous_work_id
                .map(|id| format!("Paused previous work #{id}.\n\n"))
                .unwrap_or_default();
            let started = format!("Started work #{work_id}:\n  {goal}");
            if outcome.previous_work_id.is_some() {
                format!("{paused}{started}")
            } else {
                format!("{started}\n\nActive work is now #{work_id}.")
            }
        }
        WorkOperationDto::Focus { text } => {
            format!("Updated focus for work #{work_id}:\n  {text}")
        }
        WorkOperationDto::AddEntry { kind, text } => {
            let label = match kind {
                aibe_protocol::WorkEntryKindDto::Idea => "idea",
                aibe_protocol::WorkEntryKindDto::Note => "note",
                aibe_protocol::WorkEntryKindDto::Decision => "decision",
            };
            format!("Added {label} to work #{work_id}:\n  {text}")
        }
        WorkOperationDto::Defer { .. } => {
            format!("Deferred work #{work_id}:\n  {}", work.title)
        }
        WorkOperationDto::Switch { .. } => {
            format!("Switched active work:\n  #{} {}", work.id, work.title)
        }
        WorkOperationDto::Finish => {
            format!("Finished work #{}:\n  {}", work.id, work.title)
        }
        WorkOperationDto::Push { .. } | WorkOperationDto::Pop => format!(
            "Updated work #{work_id}.\n{}",
            render_work_snapshot(snapshot, WorkView::Status)
        ),
    };
    Ok(rendered)
}

fn mutation_kind(operation: &WorkOperationDto) -> WorkMutationKindDto {
    match operation {
        WorkOperationDto::Start { .. } => WorkMutationKindDto::Start,
        WorkOperationDto::Focus { .. } => WorkMutationKindDto::Focus,
        WorkOperationDto::AddEntry { .. } => WorkMutationKindDto::AddEntry,
        WorkOperationDto::Defer { .. } => WorkMutationKindDto::Defer,
        WorkOperationDto::Switch { .. } => WorkMutationKindDto::Switch,
        WorkOperationDto::Push { .. } => WorkMutationKindDto::Push,
        WorkOperationDto::Pop => WorkMutationKindDto::Pop,
        WorkOperationDto::Finish => WorkMutationKindDto::Finish,
    }
}

fn validate_phase1_result(
    operation: &WorkOperationDto,
    snapshot: &WorkSnapshotDto,
    outcome: &aibe_protocol::WorkMutationOutcomeDto,
    work: &WorkItemDto,
) -> Result<(), AgentError> {
    let valid = match operation {
        WorkOperationDto::Start { goal } => {
            snapshot.active_work_id == Some(work.id)
                && work.status == WorkStatusDto::Active
                && work.goal == *goal
                && work.title == *goal
                && outcome.previous_work_id.is_none_or(|previous_id| {
                    previous_id != work.id
                        && snapshot.works.iter().any(|previous| {
                            previous.id == previous_id && previous.status == WorkStatusDto::Paused
                        })
                })
        }
        WorkOperationDto::Focus { text } => {
            snapshot.active_work_id == Some(work.id)
                && work.status == WorkStatusDto::Active
                && work.focus.as_deref() == Some(text)
                && outcome.previous_work_id.is_none()
        }
        WorkOperationDto::AddEntry { kind, text } => {
            snapshot.active_work_id == Some(work.id)
                && work.status == WorkStatusDto::Active
                && outcome.previous_work_id.is_none()
                && snapshot.entries.iter().any(|entry| {
                    entry.work_id == work.id && entry.kind == *kind && entry.text == *text
                })
        }
        WorkOperationDto::Defer { text } => {
            work.status == WorkStatusDto::Deferred
                && work.goal == *text
                && work.title == *text
                && outcome.previous_work_id.is_none()
        }
        WorkOperationDto::Switch { .. } => {
            snapshot.active_work_id == Some(work.id)
                && work.status == WorkStatusDto::Active
                && snapshot.stack.is_empty()
                && outcome.previous_work_id.is_none_or(|previous_id| {
                    previous_id != work.id
                        && snapshot.works.iter().any(|previous| {
                            previous.id == previous_id && previous.status == WorkStatusDto::Paused
                        })
                })
        }
        WorkOperationDto::Finish => {
            snapshot.active_work_id.is_none()
                && work.status == WorkStatusDto::Done
                && work.finished_at_ms.is_some()
                && outcome.previous_work_id.is_none()
                && snapshot.stack.is_empty()
        }
        WorkOperationDto::Push { .. } | WorkOperationDto::Pop => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

fn invalid_response() -> AgentError {
    AgentError::Request("invalid work response".into())
}
