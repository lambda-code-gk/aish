//! client-provided tool の canonical LLM 定義（client 送信 schema は信用しない）。

use serde_json::json;

use aibe_protocol::ClientProvidedToolSpec;

use crate::domain::{provider_tool_name, AISH_REPLAY_SHOW_LOGICAL, AISH_REPLAY_SHOW_PROVIDER};
use crate::ports::outbound::ToolDefinition;

pub fn client_tool_definitions(client_tools: &[ClientProvidedToolSpec]) -> Vec<ToolDefinition> {
    client_tools
        .iter()
        .filter_map(|spec| canonical_client_tool_definition(&spec.name))
        .collect()
}

pub fn canonical_client_tool_definition(logical_name: &str) -> Option<ToolDefinition> {
    if logical_name == AISH_REPLAY_SHOW_LOGICAL {
        Some(canonical_aish_replay_show_tool_definition())
    } else {
        None
    }
}

pub fn canonical_aish_replay_show_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: provider_tool_name(AISH_REPLAY_SHOW_LOGICAL)
            .unwrap_or(AISH_REPLAY_SHOW_PROVIDER)
            .to_string(),
        description:
            "Show recorded terminal output from the current replayable shell session span.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0
                },
                "stream": {
                    "type": "string",
                    "enum": ["stdout", "stderr", "both"],
                    "default": "both"
                },
                "tail_bytes": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 16384,
                    "default": 8192
                }
            },
            "required": ["index"],
            "additionalProperties": false
        }),
    }
}

/// client tool result を server-side 上限で UTF-8 安全に truncate する。
pub fn limit_client_tool_result(content: &str, max_bytes: usize) -> String {
    let max_bytes = max_bytes.min(aibe_protocol::MAX_TOOL_OUTPUT_BYTES);
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let head = &content[..end];
    format!("{head}\n\n[client tool result truncated by aibe: max_output_bytes={max_bytes}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aibe_protocol::ToolRiskClass;

    #[test]
    fn client_tool_result_clamped_by_aibe_max_output_bytes() {
        let body = "あ".repeat(100);
        let out = limit_client_tool_result(&body, 10);
        assert!(out.len() < body.len());
        assert!(out.contains("[client tool result truncated by aibe: max_output_bytes=10]"));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn client_tool_definitions_ignores_client_supplied_schema() {
        let specs = vec![ClientProvidedToolSpec {
            name: AISH_REPLAY_SHOW_LOGICAL.into(),
            description: "malicious".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "index": { "type": "string" },
                    "extra": { "type": "string" }
                }
            }),
            risk_class: ToolRiskClass::ReadOnly,
            max_output_bytes: 1024,
        }];
        let defs = client_tool_definitions(&specs);
        assert_eq!(defs.len(), 1);
        let def = &defs[0];
        assert_eq!(def.name, AISH_REPLAY_SHOW_PROVIDER);
        assert!(!def.description.contains("malicious"));
        assert_eq!(
            def.parameters
                .get("additionalProperties")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            def.parameters
                .pointer("/properties/index/type")
                .and_then(|v| v.as_str()),
            Some("integer")
        );
    }

    #[test]
    fn canonical_client_tool_definition_rejects_unknown_tools() {
        assert!(canonical_client_tool_definition("aish.evil").is_none());
        assert!(canonical_client_tool_definition("replay_show").is_none());
    }
}
