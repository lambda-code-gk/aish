//! Work CLI handler。

use aibe_protocol::{
    ClientResponse, WorkApplyResponseBody, WorkOperationDto, WorkQueryResponseBody,
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
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

pub fn run_work_apply(
    client: &dyn WorkClient,
    ctx: &MemoryCliContext,
    operation: WorkOperationDto,
) -> Result<String, AgentError> {
    match client.work_apply(&ctx.session_id, &ctx.memory_context, operation)? {
        ClientResponse::WorkApplyResult(WorkApplyResponseBody {
            snapshot, outcome, ..
        }) => Ok(format!(
            "work {:?}: #{}\n{}",
            outcome.kind,
            outcome.work_id.unwrap_or(0),
            render_work_snapshot(&snapshot, WorkView::Status)
        )),
        ClientResponse::Error { message, .. } => Err(AgentError::Request(message)),
        other => Err(AgentError::Request(format!(
            "unexpected response: {other:?}"
        ))),
    }
}
