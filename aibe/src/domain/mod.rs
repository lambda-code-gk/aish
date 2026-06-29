//! ドメインモデル（外部 I/O に依存しない）。

mod agent_turn_context;
mod capability;
mod client_cwd;
mod client_tool_names;
mod contextual_memory;
mod feature_registry;
mod llm_step;
mod memory_kind_registry;
mod memory_recipe;
mod memory_recipe_registry;
mod memory_resolver_policy;
mod memory_space;
mod memory_subscription;
mod message;
mod shell_log_tail;
pub mod test_support;
mod tool;
mod tool_execution_summary;
#[cfg(feature = "memory")]
mod work;

pub use agent_turn_context::{AgentTurnContext, ContextError};
pub use capability::{
    required_capabilities_for_memory_operations, required_capability_for_memory_operation,
    Capability,
};
pub use client_cwd::{ClientCwd, ClientCwdError};
pub use client_tool_names::{
    logical_tool_name, provider_tool_name, tool_name_for_provider, AISH_REPLAY_SHOW_LOGICAL,
    AISH_REPLAY_SHOW_PROVIDER,
};
pub use contextual_memory::{
    format_memory_block, is_standard_kind, query_matches_idea_on_demand,
    resolve_entries_for_prompt, resolve_memory_operation_add, validate_kind,
    validate_standard_kind_operation, validate_text, MemoryBlock, MemoryEntry, MemoryInjectPolicy,
    MemoryScope, MemoryStatus, MemoryValidationError, ProjectKey, ProjectKeyError,
};
pub use feature_registry::{
    actions_equivalent, baseline_feature_registry, feature_action_schema_prompt,
    EffectiveFeatureMode, FeatureDefinition, FeatureEligibilityContext, FeaturePackConfig,
    FeaturePackResolution, FeatureRegistry, FeatureRegistryError,
};
pub use llm_step::LlmStepResult;
pub(crate) use memory_kind_registry::parse_kinds_toml_str;
pub use memory_kind_registry::{
    baseline_memory_kind_registry, builtin_memory_kind_registry, KindOverride, MemoryCardinality,
    MemoryKindDefinition, MemoryKindRegistry, MemoryKindRegistryError, MemoryLifecycle,
    MemoryPromptPolicy, MemoryStalePolicy, PromptOverride,
};
pub use memory_recipe::{
    build_recipe_messages, collect_recipe_materials, parse_and_validate_recipe_output,
    MemoryRecipeError, RecipeMaterialValue, RecipeMaterials, ValidatedRecipeOutput,
    ValidatedRecipeProposal,
};
pub use memory_recipe_registry::{
    baseline_memory_recipe_registry, MemoryRecipeDefinition, MemoryRecipeRegistry,
    MemoryRecipeRegistryError, RecipeMaterialQuery, RecipeOutputContract,
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
#[cfg(feature = "memory")]
pub use work::{
    WorkEntry, WorkEntryKind, WorkItem, WorkMutationError, WorkState, WorkStateError, WorkStatus,
};

pub use aibe_protocol::{
    is_known_tool, parse_tool_names, ExecutedToolCall, ExecutedToolStatus,
    ShellExecApprovalOutcome, ToolApprovalState, ToolName, ToolRiskClass, UnknownToolError,
    GIT_DIFF, GIT_STATUS, GREP, KNOWN_TOOLS, LIST_DIR, READ_FILE, SHELL_EXEC,
};
