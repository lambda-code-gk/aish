//! max-round 到達時の終端処理 outbound port。

use async_trait::async_trait;

use crate::domain::{ChatMessage, ExecutedToolCall};

use super::{LlmError, LlmProvider, TerminationCapability};

/// 実際に使用された終端戦略（port 返り値・ログ観測用。wire protocol 非公開）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationStrategyUsed {
    SummaryPrompt,
    ConversationReplay,
}

/// port 返り値。NDJSON には載せない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminationResult {
    pub strategy: TerminationStrategyUsed,
    /// Replay 経路で LLM に渡した会話に tool メッセージが含まれていたか。
    /// SummaryPrompt 経路では常に `false`。
    pub conversation_had_tool_messages: bool,
    pub assistant: ChatMessage,
}

#[async_trait]
pub trait ToolRoundTerminator: Send + Sync {
    async fn terminate(
        &self,
        llm: &dyn LlmProvider,
        conversation: &[ChatMessage],
        executed: &[ExecutedToolCall],
        max_rounds: u32,
        capability: &TerminationCapability,
    ) -> Result<TerminationResult, LlmError>;
}
