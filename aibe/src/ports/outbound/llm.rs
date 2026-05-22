//! LLM プロバイダ outbound port。

use async_trait::async_trait;
use thiserror::Error;

use crate::domain::ChatMessage;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider failed: {0}")]
    Provider(String),
}

/// 1 ターン分のテキスト応答を生成する（ツールループは将来拡張）。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError>;
}
