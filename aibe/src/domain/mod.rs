//! ドメインモデル（外部 I/O に依存しない）。

mod client_cwd;
mod llm_step;
mod message;
mod tool;
mod tool_execution_summary;
mod tool_name;

pub use client_cwd::{ClientCwd, ClientCwdError};
pub use llm_step::LlmStepResult;
pub use message::ChatMessage;
pub use tool::{ExecutedToolCall, ExecutedToolStatus, ToolCall, ToolResult};
pub use tool_execution_summary::ToolExecutionSummary;
pub use tool_name::{
    is_known_tool, ToolName, UnknownToolError, KNOWN_TOOLS, READ_FILE, SHELL_EXEC,
};
