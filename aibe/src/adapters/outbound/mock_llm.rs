//! 開発・テスト用 LLM アダプタ（HTTP なし）。

use async_trait::async_trait;

use crate::domain::{ChatMessage, LlmStepResult, MessageRole};
use crate::ports::outbound::{LlmError, LlmProvider, ToolDefinition};

/// 最後の user メッセージをエコーするモックプロバイダ。
#[derive(Debug, Default)]
pub struct MockLlm;

impl MockLlm {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LlmProvider for MockLlm {
    async fn complete(&self, messages: &[ChatMessage]) -> Result<ChatMessage, LlmError> {
        if messages
            .iter()
            .any(|m| m.content.contains("ROUTE_TURN_JSON"))
        {
            let query = messages
                .iter()
                .rev()
                .find(|m| m.is_role(MessageRole::User))
                .and_then(extract_query_from_route_user_message)
                .unwrap_or_default();
            return Ok(ChatMessage::assistant(mock_route_json(&query)));
        }

        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.is_role(MessageRole::User))
            .map(|m| m.content.as_str())
            .unwrap_or("(no user message)");

        Ok(ChatMessage::assistant(format!(
            "[mock] received: {last_user}"
        )))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<LlmStepResult, LlmError> {
        let assistant = self.complete(messages).await?;
        let _ = tools;
        Ok(LlmStepResult {
            assistant,
            tool_calls: vec![],
        })
    }
}

fn extract_query_from_route_user_message(message: &ChatMessage) -> Option<String> {
    let json_start = message.content.find('{')?;
    let json_part = message.content.get(json_start..)?;
    let value: serde_json::Value = serde_json::from_str(json_part).ok()?;
    value
        .get("query")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn mock_route_json(query: &str) -> String {
    let lower = query.to_lowercase();
    let (route_kind, tools, shell, log_esc) = if lower.contains("error") || lower.contains("fix") {
        ("tool_assisted", r#"["read_file","grep"]"#, "false", "true")
    } else if lower.contains("run ") || lower.contains("execute") {
        ("tool_assisted", r#"["shell_exec"]"#, "true", "false")
    } else if lower.contains("continue") || lower.contains("also") {
        ("continue", "null", "false", "false")
    } else {
        ("one_shot", "null", "false", "false")
    };
    format!(
        r#"{{"route_kind":"{route_kind}","new_conversation":false,"recommended_preset":null,"recommended_tools":{tools},"log_tail_bytes":null,"require_shell_approval":{shell},"log_tail_escalation":{log_esc},"route_reason":"mock route","confidence":0.9}}"#
    )
}
