use std::sync::Arc;

use aibe_protocol::{AgentTaskApprovalAudit, ToolApprovalState};
use async_trait::async_trait;
use serde_json::Value;

use crate::application::agent_task::{AgentTaskService, AgentTaskServiceError};
use crate::domain::{AgentTaskRequest, AgentTaskResult, ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ToolExecutionContext, ToolExecutor};

pub struct AgentTaskTool {
    service: Arc<AgentTaskService>,
}

impl AgentTaskTool {
    pub fn new(service: Arc<AgentTaskService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl ToolExecutor for AgentTaskTool {
    fn name(&self) -> ToolName {
        ToolName::agent_task()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        _timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        let request = match serde_json::from_value::<AgentTaskRequest>(arguments.clone()) {
            Ok(request) => request,
            Err(_) => {
                return rejected(
                    tool_call_id,
                    arguments,
                    "invalid_arguments",
                    AgentTaskApprovalAudit::NotRequested,
                    "unknown",
                    "",
                    0,
                    "none",
                )
            }
        };
        let worker = request.worker.as_str().to_string();
        let requested_cwd = request.cwd.clone().unwrap_or_default();
        let requested_timeout = request.timeout_secs.unwrap_or(0);
        match self.service.execute(tool_call_id, request, ctx).await {
            Ok(result) => match serde_json::to_string(&result) {
                Ok(content) => (
                    audited_ok(tool_call_id, arguments, &content, &result),
                    ToolResult {
                        tool_call_id: tool_call_id.into(),
                        content,
                        is_error: false,
                    },
                ),
                Err(_) => rejected(
                    tool_call_id,
                    arguments,
                    "result_serialization_failed",
                    AgentTaskApprovalAudit::Approved,
                    &worker,
                    &requested_cwd,
                    requested_timeout,
                    "none",
                ),
            },
            Err(error) => {
                let (audit, origin) = audit_for_error(&error);
                rejected(
                    tool_call_id,
                    arguments,
                    &error.to_string(),
                    audit,
                    &worker,
                    &requested_cwd,
                    requested_timeout,
                    origin,
                )
            }
        }
    }
}

fn audit_for_error(error: &AgentTaskServiceError) -> (AgentTaskApprovalAudit, &'static str) {
    match error {
        AgentTaskServiceError::Disabled
        | AgentTaskServiceError::RecursiveDelegation
        | AgentTaskServiceError::UnknownWorker
        | AgentTaskServiceError::InvalidRequest(_)
        | AgentTaskServiceError::InvalidCwd => (AgentTaskApprovalAudit::NotRequested, "none"),
        AgentTaskServiceError::ApprovalDenied => (AgentTaskApprovalAudit::Denied, "denied"),
        AgentTaskServiceError::ApprovalUnavailable => {
            (AgentTaskApprovalAudit::Unavailable, "unavailable")
        }
        AgentTaskServiceError::ApprovalCancelled => {
            (AgentTaskApprovalAudit::Cancelled, "cancelled")
        }
        AgentTaskServiceError::ApprovalTimeout => (AgentTaskApprovalAudit::Timeout, "timeout"),
    }
}

fn audited_ok(
    tool_call_id: &str,
    arguments: &Value,
    content: &str,
    result: &AgentTaskResult,
) -> ExecutedToolCall {
    ExecutedToolCall::ok(
        tool_call_id.into(),
        ToolName::agent_task(),
        arguments.clone(),
        content.to_string(),
    )
    .with_agent_task_audit(
        AgentTaskApprovalAudit::Approved,
        &result.worker,
        &result.cwd,
        result.timeout_secs,
        &result.approval_origin,
    )
}

#[allow(clippy::too_many_arguments)]
fn rejected(
    tool_call_id: &str,
    arguments: &Value,
    message: &str,
    approval: AgentTaskApprovalAudit,
    worker: &str,
    cwd: &str,
    timeout_secs: u64,
    origin: &str,
) -> (ExecutedToolCall, ToolResult) {
    let record = ExecutedToolCall::err(
        tool_call_id.into(),
        ToolName::agent_task(),
        arguments.clone(),
        "agent_task_rejected",
        message,
    )
    .with_agent_task_audit(approval, worker, cwd, timeout_secs, origin);
    debug_assert!(match approval {
        AgentTaskApprovalAudit::NotRequested
        | AgentTaskApprovalAudit::Unavailable
        | AgentTaskApprovalAudit::Cancelled
        | AgentTaskApprovalAudit::Timeout => {
            record.approval_state == Some(ToolApprovalState::NotRequired)
        }
        AgentTaskApprovalAudit::Approved | AgentTaskApprovalAudit::Denied => {
            record.approval_state == Some(ToolApprovalState::ExplicitClientOptIn)
        }
    });
    (
        record,
        ToolResult {
            tool_call_id: tool_call_id.into(),
            content: message.into(),
            is_error: true,
        },
    )
}
