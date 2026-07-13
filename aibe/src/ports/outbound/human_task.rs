use aibe_protocol::{HumanTaskRequest, HumanTaskResult};
use async_trait::async_trait;

#[async_trait]
pub trait HumanTaskGate: Send + Sync {
    async fn execute_human_task(
        &self,
        tool_call_id: &str,
        request: HumanTaskRequest,
    ) -> Option<HumanTaskResult>;
}
