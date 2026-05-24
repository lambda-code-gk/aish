//! LLM プロバイダ outbound port。

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{ChatMessage, LlmStepResult};
use crate::ports::outbound::ToolDefinition;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LlmError {
    #[error("provider failed: {0}")]
    Provider(String),
    /// LLM が組み込みツール名以外を tool call として返した。
    #[error("unknown tool: {0}")]
    UnknownTool(String),
}

/// テキスト応答およびツール付き推論。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError>;

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError>;
}
