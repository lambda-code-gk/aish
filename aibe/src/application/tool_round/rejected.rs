//! 許可外・未実装ツール要求時の tool result（ループ継続）。

use crate::domain::{ExecutedToolCall, ToolCall, ToolResult};

/// モデルが許可外・未実装ツールを要求したときの tool result（ループ継続）。
pub(crate) fn rejected_tool_result(
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
