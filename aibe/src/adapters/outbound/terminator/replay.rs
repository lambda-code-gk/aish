//! ConversationReplay 終端戦略: ループ会話を無加工で `complete()` に渡す。

use crate::domain::ChatMessage;
use crate::ports::outbound::{LlmError, LlmProvider, TerminationResult, TerminationStrategyUsed};

/// ConversationReplay: max-round 直後のループ会話を変更せず LLM に渡す。
pub async fn conversation_replay(
    llm: &dyn LlmProvider,
    conversation: &[ChatMessage],
) -> Result<TerminationResult, LlmError> {
    let had_tool_messages = conversation.iter().any(|m| m.role == "tool");
    let assistant = llm.complete(conversation).await?;
    Ok(TerminationResult {
        strategy: TerminationStrategyUsed::ConversationReplay,
        conversation_had_tool_messages: had_tool_messages,
        assistant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::domain::LlmStepResult;
    use crate::ports::outbound::{LlmError, ToolDefinition};

    struct CapturingLlm {
        last_complete_messages: Mutex<Option<Vec<ChatMessage>>>,
    }

    impl CapturingLlm {
        fn new() -> Self {
            Self {
                last_complete_messages: Mutex::new(None),
            }
        }

        fn captured(&self) -> Vec<ChatMessage> {
            self.last_complete_messages
                .lock()
                .expect("lock")
                .clone()
                .expect("captured")
        }
    }

    #[async_trait]
    impl LlmProvider for CapturingLlm {
        async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
            *self.last_complete_messages.lock().expect("lock") = Some(messages.to_vec());
            Ok(ChatMessage::assistant("done"))
        }

        async fn complete_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<LlmStepResult, LlmError> {
            Err(LlmError::Provider("not used".into()))
        }
    }

    #[tokio::test]
    async fn replay_passes_loop_conversation_unchanged() {
        let conversation = vec![
            ChatMessage::user("read all"),
            ChatMessage::assistant("tool call"),
            ChatMessage::tool("c1", "file content"),
        ];
        let llm = CapturingLlm::new();
        let result = conversation_replay(&llm, &conversation).await.expect("ok");
        assert_eq!(result.strategy, TerminationStrategyUsed::ConversationReplay);
        assert!(result.conversation_had_tool_messages);
        assert_eq!(llm.captured(), conversation);
    }

    #[tokio::test]
    async fn replay_includes_shell_log_tail_when_present() {
        let conversation = vec![
            ChatMessage::user("[shell log tail]\nlog line"),
            ChatMessage::user("read all"),
            ChatMessage::assistant("tool call"),
            ChatMessage::tool("c1", "file content"),
        ];
        let llm = CapturingLlm::new();
        conversation_replay(&llm, &conversation).await.expect("ok");
        let captured = llm.captured();
        assert_eq!(captured.len(), 4);
        assert!(captured[0].content.starts_with("[shell log tail]\n"));
    }
}
