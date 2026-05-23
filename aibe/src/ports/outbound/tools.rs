//! ツール実行 outbound port。

use async_trait::async_trait;
use serde_json::Value;

use super::tool_context::ToolExecutionContext;
use crate::domain::{ExecutedToolCall, ToolResult};

/// LLM に渡すツール定義。
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 1 回のツール実行。
///
/// 相対パス・作業ディレクトリが必要なツールは [`ToolExecutionContext::base_dir`] /
/// [`ToolExecutionContext::resolve_path`] を使う。aibe の [`std::env::current_dir`] を直接使わない。
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn name(&self) -> &'static str;

    async fn execute(
        &self,
        tool_call_id: &str,
        arguments: &Value,
        timeout_ms: u64,
        ctx: &ToolExecutionContext,
    ) -> (ExecutedToolCall, ToolResult);
}
