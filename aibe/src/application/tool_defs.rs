//! 組み込みツールの LLM 定義。

use serde_json::json;

use aibe_protocol::SHELL_EXEC;

use crate::domain::ToolName;
use crate::ports::outbound::ToolDefinition;

pub use crate::application::client_tool_defs::client_tool_definitions;
pub use crate::domain::{
    logical_tool_name, provider_tool_name, tool_name_for_provider, AISH_REPLAY_SHOW_LOGICAL,
    AISH_REPLAY_SHOW_PROVIDER,
};

pub use crate::domain::{is_known_tool, KNOWN_TOOLS, READ_FILE};
pub use crate::domain::{GIT_DIFF, GIT_STATUS, GREP, LIST_DIR};

pub fn definitions_for(allowed: &[ToolName]) -> Vec<ToolDefinition> {
    allowed
        .iter()
        .filter_map(|name| match name.as_str() {
            SHELL_EXEC => Some(shell_exec_definition()),
            READ_FILE => Some(read_file_definition()),
            LIST_DIR => Some(list_dir_definition()),
            GREP => Some(grep_definition()),
            GIT_DIFF => Some(git_diff_definition()),
            GIT_STATUS => Some(git_status_definition()),
            _ => None,
        })
        .collect()
}

fn shell_exec_definition() -> ToolDefinition {
    ToolDefinition {
        name: SHELL_EXEC.to_string(),
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
        name: READ_FILE.to_string(),
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

fn list_dir_definition() -> ToolDefinition {
    ToolDefinition {
        name: LIST_DIR.to_string(),
        description: "List directory contents without running a shell command.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to the client cwd"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Whether to recurse into subdirectories"
                }
            }
        }),
    }
}

fn grep_definition() -> ToolDefinition {
    ToolDefinition {
        name: GREP.to_string(),
        description: "Search text files with a regular expression without spawning a shell."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory path relative to the client cwd"
                }
            },
            "required": ["pattern"]
        }),
    }
}

fn git_diff_definition() -> ToolDefinition {
    ToolDefinition {
        name: GIT_DIFF.to_string(),
        description: "Show git diff output for the current repository without mutating state."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional path inside the repository"
                }
            }
        }),
    }
}

fn git_status_definition() -> ToolDefinition {
    ToolDefinition {
        name: GIT_STATUS.to_string(),
        description: "Show git status output for the current repository without mutating state."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional path inside the repository"
                }
            }
        }),
    }
}
