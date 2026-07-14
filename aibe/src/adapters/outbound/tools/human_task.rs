use crate::domain::{ExecutedToolCall, ToolName, ToolResult};
use crate::ports::outbound::{ToolExecutionContext, ToolExecutor};
use aibe_protocol::{ExecutionMode, HumanTaskRequest};
use async_trait::async_trait;
use serde_json::Value;

pub struct HumanTaskTool;

fn failure(
    id: String,
    arguments: Value,
    code: &str,
    message: &str,
) -> (ExecutedToolCall, ToolResult) {
    (
        ExecutedToolCall::err(
            id.clone(),
            aibe_protocol::HUMAN_TASK,
            arguments,
            code,
            message,
        ),
        ToolResult {
            tool_call_id: id,
            content: message.into(),
            is_error: true,
        },
    )
}

#[async_trait]
impl ToolExecutor for HumanTaskTool {
    fn name(&self) -> ToolName {
        ToolName::human_task()
    }

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        _timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult) {
        let id = tool_call_id.to_string();
        if ctx.execution_mode() != ExecutionMode::Collaborative {
            return failure(
                id,
                arguments.clone(),
                "tool_not_allowed",
                "human_task is available only in Collaborative Mode",
            );
        }
        let parsed: HumanTaskRequest = match serde_json::from_value(arguments.clone()) {
            Ok(value) => value,
            Err(_) => {
                return failure(
                    id,
                    arguments.clone(),
                    "invalid_arguments",
                    "invalid human_task arguments",
                )
            }
        };
        let request = match parsed.normalized() {
            Ok(value) => value,
            Err(_) => {
                return failure(
                    id,
                    arguments.clone(),
                    "invalid_arguments",
                    "invalid human_task arguments",
                )
            }
        };
        let Some(gate) = ctx.human_task_gate() else {
            return failure(
                id,
                arguments.clone(),
                "human_task_unavailable",
                "interactive human task client is unavailable",
            );
        };
        let Some(result) = gate.execute_human_task(tool_call_id, request.clone()).await else {
            return failure(
                id,
                arguments.clone(),
                "human_task_unavailable",
                "human task callback failed",
            );
        };
        if result.validate().is_err() {
            return failure(
                id,
                arguments.clone(),
                "human_task_unavailable",
                "invalid human task result",
            );
        }
        // 開始時 request の clone 不変条件。client による task 改変を fail-closed にする。
        if result.task != request {
            return failure(
                id,
                arguments.clone(),
                "human_task_unavailable",
                "human task result task does not match the start request",
            );
        }
        let content = match serde_json::to_string(&result) {
            Ok(value) => value,
            Err(_) => {
                return failure(
                    id,
                    arguments.clone(),
                    "human_task_unavailable",
                    "human task result serialization failed",
                )
            }
        };
        (
            ExecutedToolCall::ok(id.clone(), self.name(), arguments.clone(), content.clone()),
            ToolResult {
                tool_call_id: id,
                content,
                is_error: false,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ClientCwd;
    use crate::ports::outbound::HumanTaskGate;
    use aibe_protocol::{
        HandoffExecutionOutcome, HumanTaskEvidence, HumanTaskResult, PostHandoffObservation,
        ShellLogRange,
    };
    use std::sync::Arc;

    struct Gate;
    #[async_trait]
    impl HumanTaskGate for Gate {
        async fn execute_human_task(
            &self,
            _: &str,
            request: HumanTaskRequest,
        ) -> Option<HumanTaskResult> {
            Some(HumanTaskResult {
                status: HandoffExecutionOutcome::Done,
                task: request,
                human_shell_exit_code: Some(0),
                final_shell_cwd: Some("/tmp".into()),
                shell_log_range: Some(ShellLogRange {
                    start: 0,
                    end: Some(1),
                }),
                observation: Some(PostHandoffObservation {
                    cwd_exists: true,
                    cwd: "/tmp".into(),
                    git_head: None,
                    git_branch: None,
                    git_status: None,
                    shell_log_tail: None,
                    shell_log_truncated: None,
                    observation_errors: Vec::new(),
                    human_task_evidence: Some(HumanTaskEvidence {
                        commands: Vec::new(),
                        truncated: false,
                    }),
                }),
                error: None,
            })
        }
    }

    fn ctx(mode: ExecutionMode) -> ToolExecutionContext {
        ToolExecutionContext::new(ClientCwd::parse("/tmp").unwrap())
            .with_execution_mode(mode)
            .with_human_task_gate(Arc::new(Gate))
    }

    #[tokio::test]
    async fn rejects_normal_mode_even_with_registered_executor() {
        let (_, result) = HumanTaskTool
            .execute(
                "c",
                &serde_json::json!({"objective":"x"}),
                1,
                &ctx(ExecutionMode::Normal),
            )
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("Collaborative Mode"));
    }

    #[tokio::test]
    async fn collaborative_mode_returns_structured_result() {
        let (_, result) = HumanTaskTool
            .execute(
                "c",
                &serde_json::json!({"objective":" x "}),
                1,
                &ctx(ExecutionMode::Collaborative),
            )
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("\"status\":\"done\""));
    }

    struct TamperingGate;
    #[async_trait]
    impl HumanTaskGate for TamperingGate {
        async fn execute_human_task(
            &self,
            _: &str,
            request: HumanTaskRequest,
        ) -> Option<HumanTaskResult> {
            let mut tampered = request;
            tampered.objective = "rewritten-by-client".into();
            Some(HumanTaskResult {
                status: HandoffExecutionOutcome::Done,
                task: tampered,
                human_shell_exit_code: Some(0),
                final_shell_cwd: Some("/tmp".into()),
                shell_log_range: Some(ShellLogRange {
                    start: 0,
                    end: Some(1),
                }),
                observation: Some(PostHandoffObservation {
                    cwd_exists: true,
                    cwd: "/tmp".into(),
                    git_head: None,
                    git_branch: None,
                    git_status: None,
                    shell_log_tail: None,
                    shell_log_truncated: None,
                    observation_errors: Vec::new(),
                    human_task_evidence: Some(HumanTaskEvidence {
                        commands: Vec::new(),
                        truncated: false,
                    }),
                }),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn rejects_result_when_task_is_rewritten_by_client() {
        let ctx = ToolExecutionContext::new(ClientCwd::parse("/tmp").unwrap())
            .with_execution_mode(ExecutionMode::Collaborative)
            .with_human_task_gate(Arc::new(TamperingGate));
        let (_, result) = HumanTaskTool
            .execute("c", &serde_json::json!({"objective":"original"}), 1, &ctx)
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("does not match"));
        assert!(!result.content.contains("rewritten-by-client"));
    }
}
