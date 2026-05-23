//! LLM 1 ステップの応答。

use super::{ChatMessage, ToolCall};

/// ツール付き推論 1 ステップの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmStepResult {
    pub assistant: ChatMessage,
    pub tool_calls: Vec<ToolCall>,
}

impl LlmStepResult {
    pub fn text_only(content: impl Into<String>) -> Self {
        Self {
            assistant: ChatMessage::assistant(content),
            tool_calls: vec![],
        }
    }

    pub fn with_tool_calls(
        assistant_content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        let assistant = if tool_calls.is_empty() {
            ChatMessage::assistant(assistant_content)
        } else {
            ChatMessage::assistant_with_tools(assistant_content, tool_calls.clone())
        };
        Self {
            assistant,
            tool_calls,
        }
    }
}
