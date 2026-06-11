//! aibe ↔ クライアント間の wire 契約（NDJSON / serde）。

mod executed_tool;
mod memory;
mod request;
mod response;
mod tool_name;

pub use executed_tool::{
    ExecutedToolCall, ExecutedToolStatus, ShellExecApprovalOutcome, ToolApprovalState,
    ToolRiskClass,
};
pub use memory::{
    MemoryApplyRequestBody, MemoryApplyStatus, MemoryContext, MemoryEntryDto,
    MemoryInjectPolicyDto, MemoryOperationDto, MemoryQueryDto, MemoryQueryRequestBody,
    MemoryQueryStatus, MemoryScopeDto, MemoryStatusDto, MEMORY_PROMPT_BUDGET_BYTES,
    MEMORY_TEXT_MAX_BYTES,
};
pub use request::{ClientRequest, ProtocolMessage, RequestContext};
pub use request::{RouteTurnCliOverrides, RouteTurnConversation, RouteTurnSession};
pub use response::{
    AgentTurnStatus, ClientResponse, ErrorCode, ProgressPhase, ProtocolMessageOut, RouteKind,
    RoutePlan, RouteTurnResult, RouteTurnStatus,
};
pub use tool_name::{
    is_known_tool, parse_tool_names, ToolName, UnknownToolError, GIT_DIFF, GIT_STATUS, GREP,
    KNOWN_TOOLS, LIST_DIR, READ_FILE, SHELL_EXEC,
};

/// `RequestContext.shell_log_tail` の truncate 上限（バイト）。
pub const SHELL_LOG_TAIL_MAX_BYTES: usize = 16 * 1024;

/// `RequestContext.system_instruction` の truncate 上限（バイト）。
pub const SYSTEM_INSTRUCTION_MAX_BYTES: usize = 8 * 1024;

/// `tool_calls` / クライアント表示 truncate の共有上限（バイト）。
pub const MAX_TOOL_OUTPUT_BYTES: usize = 32_768;
