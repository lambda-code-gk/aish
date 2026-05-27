//! ツール上限到達時に LLM へ渡す実行記録の要約。

use crate::domain::{ExecutedToolCall, ExecutedToolStatus};

/// 実行済みツール呼び出しのプレーンテキスト要約。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionSummary(String);

impl ToolExecutionSummary {
    pub fn from_executed(calls: &[ExecutedToolCall]) -> Self {
        if calls.is_empty() {
            return Self("(no tools were executed in this turn)".to_string());
        }

        let body = calls
            .iter()
            .enumerate()
            .map(|(i, call)| format_call_block(i + 1, call))
            .collect::<Vec<_>>()
            .join("\n");
        Self(body)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_prompt_section(self, max_rounds: u32) -> String {
        format!(
            "## Tool execution results (maximum tool rounds {max_rounds} reached)\n\n\
             {}\n\n\
             Respond to the user's request above.\n\
             - If any tool has status ok with output, you MUST use that content in your answer.\n\
             - Do not claim files were completely unreadable when partial successful reads exist.\n\
             - Mention briefly that the tool round limit was reached.",
            self.0
        )
    }
}

fn format_call_block(index: usize, call: &ExecutedToolCall) -> String {
    let mut block = format!(
        "### {index}. {} (id: {})\n- arguments: {}\n",
        call.name, call.id, call.arguments
    );
    match call.status {
        ExecutedToolStatus::Ok => {
            block.push_str("- status: ok\n- output:\n");
            block.push_str(call.output.as_deref().unwrap_or("(empty)"));
        }
        ExecutedToolStatus::Error => {
            block.push_str("- status: error\n");
            if let Some(code) = &call.error {
                block.push_str(&format!("- error: {code}\n"));
            }
            if let Some(msg) = &call.message {
                block.push_str(&format!("- message: {msg}\n"));
            }
        }
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ToolName;
    use serde_json::json;

    #[test]
    fn includes_ok_output_and_errors() {
        let calls = vec![
            ExecutedToolCall::ok(
                "c1".into(),
                ToolName::read_file(),
                json!({"path": "a.md"}),
                "line one".into(),
            ),
            ExecutedToolCall::err(
                "c2".into(),
                ToolName::read_file(),
                json!({"path": "b.md"}),
                "path_not_allowed",
                "path is outside allowed_roots",
            ),
        ];
        let summary = ToolExecutionSummary::from_executed(&calls);
        assert!(summary.as_str().contains("line one"));
        assert!(summary.as_str().contains("status: ok"));
        assert!(summary.as_str().contains("status: error"));
        assert!(summary.as_str().contains("path is outside allowed_roots"));
    }
}
