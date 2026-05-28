//! aibe ↔ クライアント間の wire 契約（NDJSON / serde）。

mod executed_tool;
mod request;
mod response;
mod tool_name;

pub use executed_tool::{ExecutedToolCall, ExecutedToolStatus, ToolApprovalState, ToolRiskClass};
pub use request::{ClientRequest, ProtocolMessage, RequestContext};
pub use response::{AgentTurnStatus, ClientResponse, ErrorCode, ProtocolMessageOut};
pub use tool_name::{
    is_known_tool, parse_tool_names, ToolName, UnknownToolError, GIT_DIFF, GIT_STATUS, GREP,
    KNOWN_TOOLS, LIST_DIR, READ_FILE, SHELL_EXEC,
};

/// `RequestContext.shell_log_tail` の truncate 上限（バイト）。
pub const SHELL_LOG_TAIL_MAX_BYTES: usize = 16 * 1024;

/// `tool_calls` / クライアント表示 truncate の共有上限（バイト）。
pub const MAX_TOOL_OUTPUT_BYTES: usize = 32_768;
