//! 許可外・未実装ツール要求時の tool result（ループ継続）。

use crate::domain::{sanitize_tool_arguments_for_audit, ExecutedToolCall, ToolCall, ToolResult};

/// モデルが許可外・未実装ツールを要求したときの tool result（ループ継続）。
pub(crate) fn rejected_tool_result(
    tc: &ToolCall,
    error: &str,
    message: String,
) -> (ExecutedToolCall, ToolResult) {
    let sanitized = sanitize_tool_arguments_for_audit(tc.name.as_str(), &tc.arguments);
    let record = ExecutedToolCall::err(
        tc.id.clone(),
        tc.name.clone(),
        sanitized,
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
