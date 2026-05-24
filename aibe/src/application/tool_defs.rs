//! 組み込みツールの LLM 定義。

use serde_json::json;

use crate::domain::ToolName;
use crate::ports::outbound::ToolDefinition;

pub use crate::domain::{is_known_tool, KNOWN_TOOLS, READ_FILE, SHELL_EXEC};

pub fn definitions_for(allowed: &[ToolName]) -> Vec<ToolDefinition> {
    allowed
        .iter()
        .filter_map(|name| match name.as_str() {
            SHELL_EXEC => Some(shell_exec_definition()),
            READ_FILE => Some(read_file_definition()),
            _ => None,
        })
        .collect()
}

fn shell_exec_definition() -> ToolDefinition {
    ToolDefinition {
        name: ToolName::shell_exec(),
        description: "Run a subprocess command. Only commands listed in server config are allowed."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Executable name or path" },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional arguments"
                }
            },
            "required": ["command"]
        }),
    }
}

fn read_file_definition() -> ToolDefinition {
    ToolDefinition {
        name: ToolName::read_file(),
        description: "Read a text file under configured allowed roots.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to allowed root or absolute under root" },
                "offset": { "type": "integer", "description": "1-based line to start reading" },
                "limit": { "type": "integer", "description": "Maximum lines to read" }
            },
            "required": ["path"]
        }),
    }
}
