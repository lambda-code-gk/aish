//! ツールラウンド上限到達時の終端処理（port 委譲）。

use tracing::info;

use crate::application::llm_error::client_response_for_llm_error;
use crate::application::tool_defs::SET_TASK_CONTRACT;
use crate::domain::{ChatMessage, ExecutedToolCall};
use crate::ports::outbound::{LlmProvider, TerminationCapability, ToolRoundTerminator};
use aibe_protocol::{AgentTurnStatus, ClientResponse};

use crate::application::protocol_convert::protocol_message_out_from_chat;

/// terminator 出力はそのまま client 応答本文になるため、control args は
/// provider wire sink ではなく redact 必須 sink として扱う（spec 0073 §12）。
fn redact_task_contract_control_args(conversation: &[ChatMessage]) -> Vec<ChatMessage> {
    conversation
        .iter()
        .map(|message| {
            let needs_redaction = message
                .tool_calls
                .as_ref()
                .is_some_and(|calls| calls.iter().any(|call| call.name == SET_TASK_CONTRACT));
            if !needs_redaction {
                return message.clone();
            }
            let mut message = message.clone();
            if let Some(calls) = message.tool_calls.as_mut() {
                for call in calls.iter_mut().filter(|c| c.name == SET_TASK_CONTRACT) {
                    call.arguments = serde_json::json!({"redacted": true});
                    call.provider_extras = None;
                }
            }
            message
        })
        .collect()
}

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
    let conversation = redact_task_contract_control_args(conversation);
    match terminator
        .terminate(llm, &conversation, &executed, max_rounds, capability)
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
                completion_report: None,
            }
        }
        Err(e) => client_response_for_llm_error(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ToolCall;
    use serde_json::json;

    #[test]
    fn control_args_are_redacted_before_terminator_input() {
        let conversation = vec![
            ChatMessage::user("create artifact"),
            ChatMessage::assistant_with_tools(
                "",
                vec![
                    ToolCall {
                        id: "c1".into(),
                        name: SET_TASK_CONTRACT.into(),
                        arguments: json!({"goal": "sk-secret-goal"}),
                        provider_extras: Some(json!({"raw": "sk-secret-goal"})),
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "read_file".into(),
                        arguments: json!({"path": "artifact.txt"}),
                        provider_extras: None,
                    },
                ],
            ),
            ChatMessage::tool("c1", r#"{"status":"contract_fixed"}"#),
        ];

        let redacted = redact_task_contract_control_args(&conversation);

        let calls = redacted[1].tool_calls.as_ref().expect("tool calls");
        assert_eq!(calls[0].arguments, json!({"redacted": true}));
        assert!(calls[0].provider_extras.is_none());
        assert_eq!(calls[1].arguments, json!({"path": "artifact.txt"}));
        assert!(!serde_json::to_string(&redacted)
            .expect("serialize")
            .contains("sk-secret-goal"));
    }
}
