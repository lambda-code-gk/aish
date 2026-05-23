//! `agent_turn` ユースケース（ツール付きエージェントループ）。

use std::sync::Arc;

use crate::application::tool_defs::{definitions_for, is_known_tool};
use crate::domain::{ChatMessage, ExecutedToolCall, ToolCall, ToolResult};
use crate::ports::outbound::{LlmError, LlmProvider, ToolRegistry, ToolsConfig};
use crate::protocol::RequestContext;
use crate::protocol::{ClientResponse, ErrorCode, ProtocolMessageOut};

pub struct AgentTurnService {
    llm: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    tools_config: ToolsConfig,
}

impl AgentTurnService {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        registry: Arc<dyn ToolRegistry>,
        tools_config: ToolsConfig,
    ) -> Self {
        Self {
            llm,
            registry,
            tools_config,
        }
    }

    pub async fn run(
        &self,
        id: String,
        messages: Vec<ChatMessage>,
        tools: Vec<String>,
        context: RequestContext,
    ) -> ClientResponse {
        if messages.is_empty() {
            return ClientResponse::error(
                id,
                ErrorCode::InvalidRequest,
                "messages must not be empty",
            );
        }

        for name in &tools {
            if !is_known_tool(name) {
                return ClientResponse::error(
                    id.clone(),
                    ErrorCode::ToolNotAllowed,
                    format!("unknown tool: {name}"),
                );
            }
        }

        let mut conversation = messages;
        if let Some(tail) = context.shell_log_tail {
            if !tail.is_empty() {
                conversation.insert(0, ChatMessage::user(format!("[shell log tail]\n{tail}")));
            }
        }

        if tools.is_empty() {
            return self.finish_text_only(id, &conversation).await;
        }

        self.run_with_tools(id, conversation, tools).await
    }

    async fn finish_text_only(&self, id: String, conversation: &[ChatMessage]) -> ClientResponse {
        match self.llm.complete(conversation).await {
            Ok(assistant) => ClientResponse::AgentTurnResult {
                id,
                status: "ok".to_string(),
                assistant_message: ProtocolMessageOut::from_assistant(&assistant),
                tool_calls: vec![],
            },
            Err(LlmError::Provider(msg)) => {
                ClientResponse::error(id, ErrorCode::ProviderError, msg)
            }
        }
    }

    async fn run_with_tools(
        &self,
        id: String,
        mut conversation: Vec<ChatMessage>,
        allowed_tools: Vec<String>,
    ) -> ClientResponse {
        let tool_defs = definitions_for(&allowed_tools);
        let mut executed: Vec<ExecutedToolCall> = Vec::new();
        let max_rounds = self.tools_config.max_rounds.max(1);

        for round in 0..max_rounds {
            let step = match self
                .llm
                .complete_with_tools(&conversation, &tool_defs)
                .await
            {
                Ok(s) => s,
                Err(LlmError::Provider(msg)) => {
                    return ClientResponse::error(id, ErrorCode::ProviderError, msg);
                }
            };

            if step.tool_calls.is_empty() {
                return ClientResponse::AgentTurnResult {
                    id,
                    status: "ok".to_string(),
                    assistant_message: ProtocolMessageOut::from_assistant(&step.assistant),
                    tool_calls: executed.into_iter().map(|e| e.to_json()).collect(),
                };
            }

            conversation.push(step.assistant.clone());

            for tc in &step.tool_calls {
                let (record, result) = if !allowed_tools.iter().any(|t| t == &tc.name) {
                    rejected_tool_result(
                        tc,
                        "tool_not_allowed",
                        format!("model requested disallowed tool: {}", tc.name),
                    )
                } else if let Some(executor) = self.registry.get(&tc.name) {
                    executor
                        .execute(&tc.id, &tc.arguments, self.tools_config.exec_timeout_ms)
                        .await
                } else {
                    rejected_tool_result(
                        tc,
                        "tool_not_implemented",
                        format!("tool not implemented: {}", tc.name),
                    )
                };
                executed.push(record);
                let content = if result.is_error {
                    format!("[tool error]\n{}", result.content)
                } else {
                    result.content
                };
                conversation.push(ChatMessage::tool(tc.id.clone(), content));
            }

            if round + 1 >= max_rounds {
                return self
                    .finish_after_max_tool_rounds(id, &conversation, executed, max_rounds)
                    .await;
            }
        }

        ClientResponse::error(
            id,
            ErrorCode::InternalError,
            "agent loop ended unexpectedly",
        )
    }

    /// ツール上限到達後、取得済み tool result だけを根拠に最終応答を生成する。
    ///
    /// 最終 `complete()` は `tools` なしのため、一部プロバイダは会話中の `role: tool`
    /// メッセージを無視する。成功・失敗の実行記録を本文に埋め込んで渡す。
    async fn finish_after_max_tool_rounds(
        &self,
        id: String,
        conversation: &[ChatMessage],
        executed: Vec<ExecutedToolCall>,
        max_rounds: u32,
    ) -> ClientResponse {
        let summary = format_tool_execution_summary(&executed);
        let mut final_conversation = Vec::new();
        if let Some(user) = initial_user_request(conversation) {
            final_conversation.push(user);
        }
        final_conversation.push(ChatMessage::user(format!(
            "## Tool execution results (maximum tool rounds {max_rounds} reached)\n\n\
             {summary}\n\n\
             Respond to the user's request above.\n\
             - If any tool has status ok with output, you MUST use that content in your answer.\n\
             - Do not claim files were completely unreadable when partial successful reads exist.\n\
             - Mention briefly that the tool round limit was reached."
        )));

        match self.llm.complete(&final_conversation).await {
            Ok(assistant) => ClientResponse::AgentTurnResult {
                id,
                status: "max_tool_rounds".to_string(),
                assistant_message: ProtocolMessageOut::from_assistant(&assistant),
                tool_calls: executed.into_iter().map(|e| e.to_json()).collect(),
            },
            Err(LlmError::Provider(msg)) => {
                ClientResponse::error(id, ErrorCode::ProviderError, msg)
            }
        }
    }
}

/// モデルが許可外・未実装ツールを要求したときの tool result（ループ継続）。
fn rejected_tool_result(
    tc: &ToolCall,
    error: &str,
    message: String,
) -> (ExecutedToolCall, ToolResult) {
    let record = ExecutedToolCall::err(
        tc.id.clone(),
        tc.name.clone(),
        tc.arguments.clone(),
        error,
        message.clone(),
    );
    let result = ToolResult {
        tool_call_id: tc.id.clone(),
        content: message,
        is_error: true,
    };
    (record, result)
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

/// 最終要約用: 実行済みツールの成功出力・失敗理由をプレーンテキスト化する。
fn format_tool_execution_summary(executed: &[ExecutedToolCall]) -> String {
    if executed.is_empty() {
        return "(no tools were executed in this turn)".to_string();
    }

    executed
        .iter()
        .enumerate()
        .map(|(i, call)| {
            let mut block = format!(
                "### {}. {} (id: {})\n- arguments: {}\n",
                i + 1,
                call.name,
                call.id,
                call.arguments
            );
            if call.status == "ok" {
                block.push_str("- status: ok\n- output:\n");
                block.push_str(call.output.as_deref().unwrap_or("(empty)"));
            } else {
                block.push_str("- status: error\n");
                if let Some(code) = &call.error {
                    block.push_str(&format!("- error: {code}\n"));
                }
                if let Some(msg) = &call.message {
                    block.push_str(&format!("- message: {msg}\n"));
                }
            }
            block
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_includes_ok_output_and_errors() {
        let executed = vec![
            ExecutedToolCall::ok(
                "c1".into(),
                "read_file".into(),
                json!({"path": "a.md"}),
                "line one".into(),
            ),
            ExecutedToolCall::err(
                "c2".into(),
                "read_file".into(),
                json!({"path": "b.md"}),
                "path_not_allowed",
                "path is outside allowed_roots",
            ),
        ];
        let summary = format_tool_execution_summary(&executed);
        assert!(summary.contains("line one"));
        assert!(summary.contains("status: ok"));
        assert!(summary.contains("status: error"));
        assert!(summary.contains("path is outside allowed_roots"));
    }
}
