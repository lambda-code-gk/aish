//! ドメインモデル（外部 I/O に依存しない）。

mod agent_turn_context;
mod capability;
mod client_cwd;
mod contextual_memory;
mod llm_step;
mod memory_kind_registry;
mod memory_recipe;
mod memory_resolver_policy;
mod memory_space;
mod memory_subscription;
mod message;
mod shell_log_tail;
mod tool;
mod tool_execution_summary;

pub use agent_turn_context::{AgentTurnContext, ContextError};
pub use capability::{
    required_capabilities_for_memory_operations, required_capability_for_memory_operation,
    Capability,
};
pub use client_cwd::{ClientCwd, ClientCwdError};
pub use contextual_memory::{
    format_memory_block, is_standard_kind, query_matches_idea_on_demand,
    resolve_entries_for_prompt, resolve_memory_operation_add, validate_kind,
    validate_standard_kind_operation, validate_text, MemoryBlock, MemoryEntry, MemoryInjectPolicy,
    MemoryScope, MemoryStatus, MemoryValidationError, ProjectKey, ProjectKeyError,
    STANDARD_KIND_GOAL, STANDARD_KIND_IDEA, STANDARD_KIND_NOW,
};
pub use llm_step::LlmStepResult;
pub use memory_kind_registry::{
    builtin_memory_kind_registry, MemoryCardinality, MemoryKindDefinition, MemoryKindRegistry,
    MemoryLifecycle, MemoryPromptPolicy, MemoryStalePolicy,
};
pub use memory_recipe::{
    build_clarify_goal_messages, collect_clarify_goal_materials, parse_and_validate_recipe_output,
    ClarifyGoalMaterials, MemoryRecipeError, ValidatedRecipeOutput, ValidatedRecipeProposal,
    RECIPE_CLARIFY_GOAL,
};
pub use memory_resolver_policy::{MemoryResolveInput, MemoryResolverPolicy};
pub use memory_space::{
    now_freshness, resolve_memory_space, MemoryFreshness, MemorySpaceId, MemorySpaceResolution,
    MemorySpaceSource,
};
pub use memory_subscription::{
    change_kind_for_operation, memory_change_events_from_entries, publish_memory_changes,
    MemoryChangeEvent, MemorySubscriptionFilter,
};
pub use message::{ChatMessage, MessageRole, UnknownMessageRole};
pub use shell_log_tail::ShellLogTail;
pub use tool::{ToolCall, ToolResult};
pub use tool_execution_summary::ToolExecutionSummary;

pub use aibe_protocol::{
    is_known_tool, parse_tool_names, ExecutedToolCall, ExecutedToolStatus,
    ShellExecApprovalOutcome, ToolApprovalState, ToolName, ToolRiskClass, UnknownToolError,
    GIT_DIFF, GIT_STATUS, GREP, KNOWN_TOOLS, LIST_DIR, READ_FILE, SHELL_EXEC,
};
