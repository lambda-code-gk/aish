//! aibe ↔ クライアント間の wire 契約（NDJSON / serde）。

mod client_tool_validation;
mod collaborative_handoff;
mod executed_tool;
mod memory;
mod memory_space;
mod request;
mod response;
mod tool_name;
mod work;

pub use client_tool_validation::{validate_client_tool_arguments, validate_client_tool_call};
pub use collaborative_handoff::{
    is_safe_suggested_command, HandoffExecutionOutcome, HumanHandoffFailure, HumanHandoffResult,
    HumanTaskBriefing, HumanTaskCommandEvidence, HumanTaskEvidence, HumanTaskRequest,
    HumanTaskResult, PostHandoffObservation, RequestedCommandCompletion, ShellLogRange,
    HUMAN_TASK_BRIEFING_MAX_BYTES, SUGGESTED_COMMAND_MAX_BYTES,
};
pub use executed_tool::{
    AgentTaskApprovalAudit, ExecutedToolCall, ExecutedToolStatus, FileWriteApprovalOutcome,
    ShellExecApprovalOutcome, ToolApprovalState, ToolRiskClass,
};
pub use memory::{
    MemoryApplyRequestBody, MemoryApplyStatus, MemoryChangeEventDto, MemoryChangeKind,
    MemoryContext, MemoryEntryDto, MemoryInjectPolicyDto, MemoryKindDefinitionDto,
    MemoryKindListRequestBody, MemoryOperationAdd, MemoryOperationArchive,
    MemoryOperationClearKind, MemoryOperationDto, MemoryQueryDto, MemoryQueryRequestBody,
    MemoryQueryStatus, MemoryRecipeProposalDto, MemoryRecipeRunRequestBody, MemoryRecipeStatus,
    MemoryScopeDto, MemoryStatusDto, MemorySubscribeRequestBody, MemorySubscribeStatus,
    MEMORY_PROMPT_BUDGET_BYTES, MEMORY_TEXT_MAX_BYTES,
};
pub use memory_space::{
    is_valid_memory_space_id, is_valid_session_id, legacy_session_memory_space_id,
    project_memory_space_id,
};
pub use request::{
    ClientProvidedToolSpec, ClientRequest, ClientToolErrorKind, ClientToolResult,
    ClientToolResultStatus, ExecutionMode, ProtocolMessage, RequestContext,
    ShellExecApprovalOrigin, ToolApprovalOrigin,
};
pub use request::{
    RouteTurnCliOverrides, RouteTurnConversation, RouteTurnPreprocessorHints, RouteTurnSession,
};
pub use response::{
    AgentTurnStatus, ClientResponse, CompletionCriterionReport, CompletionCriterionStatus,
    CompletionEvidenceReport, CompletionEvidenceSource, CompletionGapReport, CompletionOutcome,
    CompletionReport, ErrorCode, FeatureAction, ProgressPhase, ProtocolMessageOut, RouteKind,
    RoutePlan, RouteTurnResult, RouteTurnStatus, VerificationTerminal,
};
pub use tool_name::{
    is_known_tool, map_advisory_tool_alias, parse_tool_names, sanitize_readonly_advisory_tools,
    sanitize_readonly_advisory_tools_option, ToolName, UnknownToolError, AGENT_TASK, APPLY_PATCH,
    GIT_DIFF, GIT_STATUS, GREP, HUMAN_TASK, KNOWN_TOOLS, LIST_DIR, READONLY_ADVISORY_TOOLS,
    READ_FILE, SHELL_EXEC, WRITE_FILE,
};
pub use work::{
    validate_work_id, validate_work_text, WorkApplyRequestBody, WorkApplyResponseBody,
    WorkEntryDto, WorkEntryKindDto, WorkInputError, WorkItemDto, WorkMutationKindDto,
    WorkMutationOutcomeDto, WorkOperationDto, WorkQueryRequestBody, WorkQueryResponseBody,
    WorkSnapshotDto, WorkStatusDto, WORK_SCHEMA_VERSION, WORK_TEXT_MAX_BYTES,
};

/// `RequestContext.shell_log_tail` の truncate 上限（バイト）。
pub const SHELL_LOG_TAIL_MAX_BYTES: usize = 16 * 1024;

/// `RequestContext.system_instruction` の truncate 上限（バイト）。
pub const SYSTEM_INSTRUCTION_MAX_BYTES: usize = 8 * 1024;

/// `tool_calls` / クライアント表示 truncate の共有上限（バイト）。
pub const MAX_TOOL_OUTPUT_BYTES: usize = 32_768;
