//! 開発・テスト用 LLM アダプタ（HTTP なし）。

use async_trait::async_trait;

use crate::domain::{ChatMessage, LlmStepResult};
use crate::ports::outbound::{LlmError, LlmProvider, ToolDefinition};

/// 最後の user メッセージをエコーするモックプロバイダ。
#[derive(Debug, Default)]
pub struct MockLlm;

impl MockLlm {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LlmProvider for MockLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("(no user message)");

        Ok(ChatMessage::assistant(format!(
            "[mock] received: {last_user}"
        )))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let assistant = self.complete(messages).await?;
        let _ = tools;
        Ok(LlmStepResult {
            assistant,
            tool_calls: vec![],
        })
    }
}
