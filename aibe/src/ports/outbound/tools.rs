//! ツール実行 outbound port。

use async_trait::async_trait;
use serde_json::Value;

use crate::domain::{ExecutedToolCall, ToolResult};

/// LLM に渡すツール定義。
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 1 回のツール実行。
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn name(&self) -> &'static str;

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        timeout_ms: u64,
    ) -> (ExecutedToolCall, ToolResult);
}
