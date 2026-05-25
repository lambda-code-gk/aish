//! 終端戦略の選択と Replay → Summary フォールバック。

use async_trait::async_trait;

use crate::domain::{ChatMessage, ExecutedToolCall};
use crate::ports::outbound::{
    LlmError, LlmProvider, TerminationCapability, TerminationResult, TerminationStrategy,
    ToolRoundTerminator,
};

use super::{replay, summary};

/// policy に従い SummaryPrompt / ConversationReplay を選択する `ToolRoundTerminator` 実装。
pub struct ToolRoundTerminatorOrchestrator {
    policy: TerminationStrategy,
}

impl ToolRoundTerminatorOrchestrator {
    pub fn new(policy: TerminationStrategy) -> Self {
        Self { policy }
    }

    fn should_try_replay(policy: TerminationStrategy, capability: &TerminationCapability) -> bool {
        policy == TerminationStrategy::ConversationReplay
            && capability.plain_complete_accepts_tool_role
    }
}

#[async_trait]
impl ToolRoundTerminator for ToolRoundTerminatorOrchestrator {
    async fn terminate(
        &self,
        llm: &dyn LlmProvider,
        conversation: &[ChatMessage],
        executed: &[ExecutedToolCall],
        max_rounds: u32,
        capability: &TerminationCapability,
    ) -> Result<TerminationResult, LlmError> {
        if Self::should_try_replay(self.policy, capability) {
            if let Ok(result) = replay::conversation_replay(llm, conversation).await {
                return Ok(result);
            }
            // Replay 実行時失敗 → SummaryPrompt に 1 回フォールバック
        }

        summary::summary_prompt(llm, conversation, executed, max_rounds).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::outbound::{TerminationStrategyUsed, ToolDefinition};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use crate::domain::{ExecutedToolCall, LlmStepResult, ToolName};
    use serde_json::json;

    struct StrategyTrackingLlm {
        complete_calls: AtomicUsize,
        fail_first_complete: bool,
        last_complete_len: Mutex<Option<usize>>,
    }

    impl StrategyTrackingLlm {
        fn new(fail_first_complete: bool) -> Self {
            Self {
                complete_calls: AtomicUsize::new(0),
                fail_first_complete,
                last_complete_len: Mutex::new(None),
            }
        }

        fn complete_call_count(&self) -> usize {
            self.complete_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmProvider for StrategyTrackingLlm {
        async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
            let n = self.complete_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_complete_len.lock().expect("lock") = Some(messages.len());
            if self.fail_first_complete && n == 0 {
                return Err(LlmError::Provider("replay rejected".into()));
            }
            Ok(ChatMessage::assistant("final"))
        }

        async fn complete_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
        ) -> Result<LlmStepResult, LlmError> {
            Err(LlmError::Provider("not used".into()))
        }
    }

    fn sample_executed() -> Vec<ExecutedToolCall> {
        vec![ExecutedToolCall::ok(
            "c1".into(),
            ToolName::read_file(),
            json!({"path": "a.txt"}),
            "data".into(),
        )]
    }

    fn loop_conversation() -> Vec<ChatMessage> {
        vec![
            ChatMessage::user("read"),
            ChatMessage::assistant("call"),
            ChatMessage::tool("c1", "data"),
        ]
    }

    #[tokio::test]
    async fn replay_skipped_when_capability_false() {
        let llm = StrategyTrackingLlm::new(false);
        let terminator =
            ToolRoundTerminatorOrchestrator::new(TerminationStrategy::ConversationReplay);
        let capability = TerminationCapability::summary_prompt_only();

        let result = terminator
            .terminate(
                &llm,
                &loop_conversation(),
                &sample_executed(),
                2,
                &capability,
            )
            .await
            .expect("ok");

        assert_eq!(result.strategy, TerminationStrategyUsed::SummaryPrompt);
        assert_eq!(llm.complete_call_count(), 1);
        // SummaryPrompt: shell log なし → user + summary = 2
        assert_eq!(*llm.last_complete_len.lock().expect("lock"), Some(2));
    }

    #[tokio::test]
    async fn replay_skipped_when_policy_summary_prompt() {
        let llm = StrategyTrackingLlm::new(false);
        let terminator = ToolRoundTerminatorOrchestrator::new(TerminationStrategy::SummaryPrompt);
        let capability = TerminationCapability {
            plain_complete_accepts_tool_role: true,
        };

        let result = terminator
            .terminate(
                &llm,
                &loop_conversation(),
                &sample_executed(),
                2,
                &capability,
            )
            .await
            .expect("ok");

        assert_eq!(result.strategy, TerminationStrategyUsed::SummaryPrompt);
        assert_eq!(llm.complete_call_count(), 1);
    }

    #[tokio::test]
    async fn replay_provider_error_falls_back_to_summary_once() {
        let llm = StrategyTrackingLlm::new(true);
        let terminator =
            ToolRoundTerminatorOrchestrator::new(TerminationStrategy::ConversationReplay);
        let capability = TerminationCapability {
            plain_complete_accepts_tool_role: true,
        };

        let result = terminator
            .terminate(
                &llm,
                &loop_conversation(),
                &sample_executed(),
                2,
                &capability,
            )
            .await
            .expect("ok");

        assert_eq!(result.strategy, TerminationStrategyUsed::SummaryPrompt);
        assert!(!result.conversation_had_tool_messages);
        assert_eq!(llm.complete_call_count(), 2);
    }

    #[tokio::test]
    async fn replay_succeeds_when_enabled() {
        let llm = StrategyTrackingLlm::new(false);
        let terminator =
            ToolRoundTerminatorOrchestrator::new(TerminationStrategy::ConversationReplay);
        let capability = TerminationCapability {
            plain_complete_accepts_tool_role: true,
        };

        let result = terminator
            .terminate(
                &llm,
                &loop_conversation(),
                &sample_executed(),
                2,
                &capability,
            )
            .await
            .expect("ok");

        assert_eq!(result.strategy, TerminationStrategyUsed::ConversationReplay);
        assert!(result.conversation_had_tool_messages);
        assert_eq!(llm.complete_call_count(), 1);
        assert_eq!(*llm.last_complete_len.lock().expect("lock"), Some(3));
    }
}
