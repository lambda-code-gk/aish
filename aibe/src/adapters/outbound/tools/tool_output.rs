//! ツール結果の LLM / `tool_calls` 向けサイズ制限。

use crate::ports::outbound::DEFAULT_MAX_TOOL_OUTPUT_BYTES;

const MIN_MAX_TOOL_OUTPUT_BYTES: usize = 256;
const MAX_MAX_TOOL_OUTPUT_BYTES: usize = 1_048_576;

/// 設定値を許容範囲に収める。
pub fn clamp_max_tool_output_bytes(requested: usize) -> usize {
    let requested = if requested == 0 {
        DEFAULT_MAX_TOOL_OUTPUT_BYTES
    } else {
        requested
    };
    requested.clamp(MIN_MAX_TOOL_OUTPUT_BYTES, MAX_MAX_TOOL_OUTPUT_BYTES)
}

/// `tool_calls.output` と LLM 向け `ToolResult.content` に載せる前に適用する。
pub fn limit_tool_output(content: &str, max_bytes: usize) -> String {
    let max_bytes = clamp_max_tool_output_bytes(max_bytes);
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let head = &content[..end];
    format!(
        "{head}\n\n[output truncated: {} bytes total, showing first {} bytes]",
        content.len(),
        end
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_under_limit() {
        let s = "hello";
        assert_eq!(limit_tool_output(s, 100), s);
    }

    #[test]
    fn truncates_with_footer() {
        let body = "x".repeat(500);
        let out = limit_tool_output(&body, 300);
        assert!(out.len() < body.len());
        assert!(out.contains("[output truncated: 500 bytes total"));
        assert!(out.starts_with("x"));
    }

    #[test]
    fn clamp_rejects_tiny_config() {
        assert_eq!(clamp_max_tool_output_bytes(10), MIN_MAX_TOOL_OUTPUT_BYTES);
    }
}
