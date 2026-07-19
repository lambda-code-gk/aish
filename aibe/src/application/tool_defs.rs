//! 組み込みツールの LLM 定義。

use serde_json::json;

use aibe_protocol::{HUMAN_TASK, SHELL_EXEC};

use crate::domain::ToolName;
use crate::ports::outbound::ToolDefinition;

pub use crate::application::client_tool_defs::client_tool_definitions;
pub use crate::domain::{
    logical_tool_name, provider_tool_name, tool_name_for_provider, AISH_REPLAY_SHOW_LOGICAL,
    AISH_REPLAY_SHOW_PROVIDER,
};

pub use crate::domain::{is_known_tool, KNOWN_TOOLS, READ_FILE};
pub use crate::domain::{APPLY_PATCH, GIT_DIFF, GIT_STATUS, GREP, LIST_DIR, WRITE_FILE};

pub fn definitions_for(allowed: &[ToolName]) -> Vec<ToolDefinition> {
    allowed
        .iter()
        .filter_map(|name| match name.as_str() {
            SHELL_EXEC => Some(shell_exec_definition()),
            HUMAN_TASK => Some(human_task_definition()),
            READ_FILE => Some(read_file_definition()),
            LIST_DIR => Some(list_dir_definition()),
            GREP => Some(grep_definition()),
            GIT_DIFF => Some(git_diff_definition()),
            GIT_STATUS => Some(git_status_definition()),
            WRITE_FILE => Some(write_file_definition()),
            APPLY_PATCH => Some(apply_patch_definition()),
            _ => None,
        })
        .collect()
}

fn human_task_definition() -> ToolDefinition {
    ToolDefinition {
        name: HUMAN_TASK.to_string(),
        description: "Delegate a task to the human by opening an interactive Human Shell that the user drives directly. \
                      Use this whenever finishing the request depends on the user acting, deciding, or providing input in their own terminal: \
                      they must supply or point at files, paths, a selection, or other data you do not have yet; \
                      they must run, inspect, edit, or confirm something interactively; the task needs their local environment or manual judgment; \
                      or they explicitly ask for the Human Shell. Prefer this over stalling with a chat-only question when the missing information \
                      or action is something the user would naturally provide by working in their shell. \
                      Do not use it when you can fully answer with your own tools and context, or when only a single trivial clarification is needed."
            .into(),
        parameters: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "objective": {
                    "type": "string",
                    "description": "Concrete goal the human should accomplish or provide in the Human Shell (for example, 'Provide the list of files to inspect')."
                },
                "reason": {
                    "type": "string",
                    "description": "Optional short explanation of why this needs the human (missing input, local action, or manual judgment)."
                },
                "instructions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional human-readable instructions shown under Suggested actions. They may be multiline and are never inserted into the shell prompt."
                },
                "suggested_commands": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional single-line shell command candidates for Alt+. / Alt+, insertion. Each must be non-empty, at most 4 KiB, and contain no control characters. They are never auto-executed; the human may edit, run, or ignore them."
                },
                "completion_criteria": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional conditions describing when the human task is done."
                }
            },
            "required": ["objective"]
        }),
    }
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
                "limit": { "type": "integer", "description": "Maximum lines to read" },
                "include_metadata": {
                    "type": "boolean",
                    "description": "If true, prepend a metadata line with sha256, size_bytes, line_ending, and trailing_newline for optimistic concurrency"
                }
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

fn write_file_definition() -> ToolDefinition {
    ToolDefinition {
        name: WRITE_FILE.to_string(),
        description: "Create or replace a text file under configured write allowed roots. \
                      For mode=replace, call read_file with include_metadata=true first and pass \
                      expected_sha256 from the metadata line."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to client cwd" },
                "mode": { "type": "string", "enum": ["create", "replace"], "description": "create for new files, replace for existing" },
                "content": { "type": "string", "description": "Full file content" },
                "expected_sha256": { "type": "string", "description": "Required for replace: SHA-256 of current file bytes (lowercase hex)" }
            },
            "required": ["path", "mode", "content"]
        }),
    }
}

fn apply_patch_definition() -> ToolDefinition {
    ToolDefinition {
        name: APPLY_PATCH.to_string(),
        description: "Apply a strict unified diff hunk to a single text file. \
                      Call read_file with include_metadata=true first and pass expected_sha256 \
                      from the metadata line. Patch must contain @@ hunks only (no ---/+++ headers)."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path relative to client cwd" },
                "patch": { "type": "string", "description": "Strict unified diff hunk without file headers" },
                "expected_sha256": { "type": "string", "description": "SHA-256 of current file bytes (lowercase hex)" }
            },
            "required": ["path", "patch", "expected_sha256"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_task_schema_separates_instructions_and_suggested_commands() {
        let schema = human_task_definition().parameters;
        let properties = schema["properties"].as_object().expect("properties");
        assert_eq!(properties["instructions"]["type"], "array");
        assert_eq!(properties["suggested_commands"]["type"], "array");
        assert!(properties["instructions"]["description"]
            .as_str()
            .expect("instructions description")
            .contains("never inserted"));
        assert!(properties["suggested_commands"]["description"]
            .as_str()
            .expect("suggested_commands description")
            .contains("Alt+. / Alt+,"));
    }
}
