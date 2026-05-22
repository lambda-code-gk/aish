//! 開発・テスト用 LLM アダプタ（HTTP なし）。

use async_trait::async_trait;

use crate::domain::ChatMessage;
use crate::ports::outbound::{LlmError, LlmProvider};

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
}
