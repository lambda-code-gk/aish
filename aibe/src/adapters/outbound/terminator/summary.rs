//! SummaryPrompt 終端戦略: 実行記録を要約 user メッセージに圧縮して `complete()` する。

use crate::domain::{ChatMessage, ExecutedToolCall, MessageRole, ToolExecutionSummary};
use crate::ports::outbound::{LlmError, LlmProvider, TerminationResult, TerminationStrategyUsed};

/// SummaryPrompt: system（あれば）+ 元 user 依頼 + 要約 user を LLM に渡す。
pub async fn summary_prompt(
    llm: &dyn LlmProvider,
    conversation: &[ChatMessage],
    executed: &[ExecutedToolCall],
    max_rounds: u32,
) -> Result<TerminationResult, LlmError> {
    let summary = ToolExecutionSummary::from_executed(executed);
    let mut final_conversation = system_messages(conversation);
    if let Some(user) = initial_user_request(conversation) {
        final_conversation.push(user);
    }
    final_conversation.push(ChatMessage::user(summary.into_prompt_section(max_rounds)));

    let assistant = llm.complete(&final_conversation).await?;
    Ok(TerminationResult {
        strategy: TerminationStrategyUsed::SummaryPrompt,
        conversation_had_tool_messages: false,
        assistant,
    })
}

/// ループ会話内の `role: system` を出現順のまま返す（SummaryPrompt 終端入力用）。
pub(crate) fn system_messages(conversation: &[ChatMessage]) -> Vec<ChatMessage> {
    conversation
        .iter()
        .filter(|m| m.is_role(MessageRole::System))
        .cloned()
        .collect()
}

/// ループ中の元ユーザー依頼（shell tail / システム追記 / 要約 user を除く）。
pub(crate) fn initial_user_request(conversation: &[ChatMessage]) -> Option<ChatMessage> {
    conversation
        .iter()
        .find(|m| {
            m.is_role(MessageRole::User)
                && !m.content.starts_with("[shell log tail]")
                && !m.content.starts_with("[system]")
                && !m.content.starts_with("## Tool execution results")
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ToolName;
    use serde_json::json;

    #[test]
    fn summary_prompt_builds_execution_results_user_message() {
        let calls = vec![ExecutedToolCall::ok(
            "c1".into(),
            ToolName::read_file(),
            json!({"path": "a.txt"}),
            "content".into(),
        )];
        let summary = ToolExecutionSummary::from_executed(&calls);
        let prompt = summary.into_prompt_section(3);
        assert!(prompt.starts_with("## Tool execution results"));
        assert!(prompt.contains("content"));
        assert!(prompt.contains("maximum tool rounds 3"));
    }

    #[test]
    fn summary_prompt_excludes_shell_log_tail() {
        let conversation = vec![
            ChatMessage::user("[shell log tail]\nlog line"),
            ChatMessage::user("read files"),
            ChatMessage::assistant("calling tool"),
            ChatMessage::tool("c1", "file content"),
        ];
        let user = initial_user_request(&conversation).expect("original user");
        assert_eq!(user.content, "read files");
        assert!(!user.content.starts_with("[shell log tail]"));
    }

    #[test]
    fn system_messages_preserves_order() {
        let system = ChatMessage {
            role: MessageRole::System,
            content: "You are helpful.".into(),
            tool_call_id: None,
            tool_calls: None,
        };
        let conversation = vec![
            system.clone(),
            ChatMessage::user("read files"),
            ChatMessage::assistant("calling tool"),
        ];
        let systems = system_messages(&conversation);
        assert_eq!(systems.len(), 1);
        assert_eq!(systems[0].content, "You are helpful.");
    }

    #[tokio::test]
    async fn summary_prompt_preserves_system_messages() {
        use async_trait::async_trait;
        use std::sync::Mutex;

        use crate::domain::{LlmStepResult, ToolName};
        use crate::ports::outbound::ToolDefinition;
        use serde_json::json;

        struct CapturingLlm {
            last_complete_messages: Mutex<Option<Vec<ChatMessage>>>,
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
                unimplemented!()
            }
        }

        let conversation = vec![
            ChatMessage {
                role: MessageRole::System,
                content: "You are helpful.".into(),
                tool_call_id: None,
                tool_calls: None,
            },
            ChatMessage::user("read files"),
            ChatMessage::assistant("calling tool"),
            ChatMessage::tool("c1", "file content"),
        ];
        let executed = vec![ExecutedToolCall::ok(
            "c1".into(),
            ToolName::read_file(),
            json!({"path": "a.txt"}),
            "content".into(),
        )];
        let llm = CapturingLlm {
            last_complete_messages: Mutex::new(None),
        };
        summary_prompt(&llm, &conversation, &executed, 3)
            .await
            .expect("ok");
        let captured = llm
            .last_complete_messages
            .lock()
            .expect("lock")
            .clone()
            .expect("captured");
        assert_eq!(captured.len(), 3);
        assert_eq!(captured[0].role, MessageRole::System);
        assert_eq!(captured[0].content, "You are helpful.");
        assert_eq!(captured[1].content, "read files");
        assert!(captured[2].content.starts_with("## Tool execution results"));
    }
}
