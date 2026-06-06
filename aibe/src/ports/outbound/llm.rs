//! LLM プロバイダ outbound port。

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::{ChatMessage, LlmStepResult};
use crate::ports::outbound::ToolDefinition;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LlmError {
    #[error("provider failed: {0}")]
    Provider(String),
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

    async fn complete_streaming(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<ChatMessage, LlmError> {
        let assistant = self.complete(messages).await?;
        if !assistant.content.is_empty() {
            on_delta(assistant.content.clone());
        }
        Ok(assistant)
    }

    async fn complete_with_tools_streaming(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        on_delta: &mut (dyn FnMut(String) + Send),
    ) -> Result<LlmStepResult, LlmError> {
        let step = self.complete_with_tools(messages, tools).await?;
        if !step.assistant.content.is_empty() {
            on_delta(step.assistant.content.clone());
        }
        Ok(step)
    }
}
