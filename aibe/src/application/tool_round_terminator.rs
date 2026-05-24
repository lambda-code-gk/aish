//! ツールラウンド上限到達時の終端処理。

use crate::application::llm_error::client_response_for_llm_error;
use crate::domain::{ChatMessage, ExecutedToolCall, ToolExecutionSummary};
use crate::ports::outbound::LlmProvider;
use crate::protocol::{AgentTurnStatus, ClientResponse, ProtocolMessageOut};

/// 最大ツールラウンド到達後、取得済み tool result を根拠に最終応答を生成する。
///
/// 最終 `complete()` は `tools` なしのため、一部プロバイダは会話中の `role: tool`
/// メッセージを無視する。成功・失敗の実行記録を本文に埋め込んで渡す。
pub async fn finish_after_max_tool_rounds(
    llm: &dyn LlmProvider,
    id: String,
    conversation: &[ChatMessage],
    executed: Vec<ExecutedToolCall>,
    max_rounds: u32,
) -> ClientResponse {
    let summary = ToolExecutionSummary::from_executed(&executed);
    let mut final_conversation = Vec::new();
    if let Some(user) = initial_user_request(conversation) {
        final_conversation.push(user);
    }
    final_conversation.push(ChatMessage::user(summary.into_prompt_section(max_rounds)));

    match llm.complete(&final_conversation).await {
        Ok(assistant) => ClientResponse::AgentTurnResult {
            id,
            status: AgentTurnStatus::MaxToolRounds,
            assistant_message: ProtocolMessageOut::from_assistant(&assistant),
            tool_calls: executed,
        },
        Err(e) => client_response_for_llm_error(id, e),
    }
}

/// ループ中の元ユーザー依頼（shell tail / システム追記を除く）。
fn initial_user_request(conversation: &[ChatMessage]) -> Option<ChatMessage> {
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
