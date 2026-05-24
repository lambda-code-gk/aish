//! SummaryPrompt 終端戦略: 実行記録を要約 user メッセージに圧縮して `complete()` する。

use crate::domain::{ChatMessage, ExecutedToolCall, ToolExecutionSummary};
use crate::ports::outbound::{LlmError, LlmProvider, TerminationResult, TerminationStrategyUsed};

/// SummaryPrompt: 元 user 依頼 + 要約 user を LLM に渡す。
pub async fn summary_prompt(
    llm: &dyn LlmProvider,
    conversation: &[ChatMessage],
    executed: &[ExecutedToolCall],
    max_rounds: u32,
) -> Result<TerminationResult, LlmError> {
    let summary = ToolExecutionSummary::from_executed(executed);
    let mut final_conversation = Vec::new();
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

/// ループ中の元ユーザー依頼（shell tail / システム追記 / 要約 user を除く）。
pub(crate) fn initial_user_request(conversation: &[ChatMessage]) -> Option<ChatMessage> {
    conversation
        .iter()
        .find(|m| {
            m.role == "user"
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
}
