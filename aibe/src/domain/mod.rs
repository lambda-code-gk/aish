//! ドメインモデル（外部 I/O に依存しない）。

mod agent_turn_context;
mod client_cwd;
mod llm_step;
mod message;
mod shell_log_tail;
mod tool;
mod tool_execution_summary;

pub use agent_turn_context::{AgentTurnContext, ContextError};
pub use client_cwd::{ClientCwd, ClientCwdError};
pub use llm_step::LlmStepResult;
pub use message::{ChatMessage, MessageRole, UnknownMessageRole};
pub use shell_log_tail::ShellLogTail;
pub use tool::{ToolCall, ToolResult};
pub use tool_execution_summary::ToolExecutionSummary;

pub use aibe_protocol::{
    is_known_tool, parse_tool_names, ExecutedToolCall, ExecutedToolStatus,
    ShellExecApprovalOutcome, ToolApprovalState, ToolName, ToolRiskClass, UnknownToolError,
    GIT_DIFF, GIT_STATUS, GREP, KNOWN_TOOLS, LIST_DIR, READ_FILE, SHELL_EXEC,
};
