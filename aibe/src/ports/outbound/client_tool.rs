//! client-provided tool の turn-local gateway。

use async_trait::async_trait;

use aibe_protocol::{ClientToolResult, ClientToolResultStatus};

#[async_trait]
pub trait ClientToolGate: Send + Sync {
    async fn request_client_tool(
        &self,
        call_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Option<ClientToolResult>;
}

pub fn empty_tool_result(
    id: String,
    turn_id: String,
    call_id: String,
    content: impl Into<String>,
) -> ClientToolResult {
    ClientToolResult {
        id,
        turn_id,
        call_id,
        status: ClientToolResultStatus::Error,
        error_kind: None,
        content: content.into(),
    }
}
