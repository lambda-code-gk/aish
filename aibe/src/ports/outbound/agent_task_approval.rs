//! Agent Task 固有の実行前承認。

use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskApprovalPrompt {
    pub worker: String,
    pub cwd: String,
    pub timeout_secs: u64,
    pub permission_profile: String,
    pub objective: String,
    pub trust_boundary_warning: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTaskApprovalOutcome {
    Approved { origin: String },
    Denied { origin: String },
    Unavailable,
    Cancelled,
    Timeout,
}

#[async_trait]
pub trait AgentTaskApprovalGate: Send + Sync {
    async fn request_agent_task_approval(
        &self,
        tool_call_id: &str,
        prompt: AgentTaskApprovalPrompt,
    ) -> AgentTaskApprovalOutcome;
}
