//! ドメインモデル（外部 I/O に依存しない）。

mod agent_turn_context;
mod client_cwd;
mod llm_step;
mod message;
mod shell_log_tail;
mod tool;
mod tool_execution_summary;
mod tool_name;

pub use agent_turn_context::{AgentTurnContext, ContextError};
pub use client_cwd::{ClientCwd, ClientCwdError};
pub use llm_step::LlmStepResult;
pub use message::{ChatMessage, MessageRole, UnknownMessageRole};
pub use shell_log_tail::ShellLogTail;
pub use tool::{ExecutedToolCall, ExecutedToolStatus, ToolCall, ToolResult};
pub use tool_execution_summary::ToolExecutionSummary;
pub use tool_name::{
    is_known_tool, parse_tool_names, ToolName, UnknownToolError, KNOWN_TOOLS, READ_FILE, SHELL_EXEC,
};
