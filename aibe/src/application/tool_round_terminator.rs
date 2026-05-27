//! ツールラウンド上限到達時の終端処理（port 委譲）。

use tracing::info;

use crate::application::llm_error::client_response_for_llm_error;
use crate::domain::{ChatMessage, ExecutedToolCall};
use crate::ports::outbound::{LlmProvider, TerminationCapability, ToolRoundTerminator};
use aibe_protocol::{AgentTurnStatus, ClientResponse};

use crate::application::protocol_convert::protocol_message_out_from_chat;

/// 最大ツールラウンド到達後、terminator port 経由で最終応答を生成する。
pub async fn finish_after_max_tool_rounds(
    llm: &dyn LlmProvider,
    terminator: &dyn ToolRoundTerminator,
    capability: &TerminationCapability,
    id: String,
    conversation: &[ChatMessage],
    executed: Vec<ExecutedToolCall>,
    max_rounds: u32,
) -> ClientResponse {
    match terminator
        .terminate(llm, conversation, &executed, max_rounds, capability)
        .await
    {
        Ok(result) => {
            info!(
                strategy = ?result.strategy,
                conversation_had_tool_messages = result.conversation_had_tool_messages,
                "max tool rounds termination"
            );
            ClientResponse::AgentTurnResult {
                id,
                status: AgentTurnStatus::MaxToolRounds,
                assistant_message: protocol_message_out_from_chat(&result.assistant),
                tool_calls: executed,
            }
        }
        Err(e) => client_response_for_llm_error(id, e),
    }
}
