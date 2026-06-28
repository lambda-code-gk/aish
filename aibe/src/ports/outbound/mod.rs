pub mod capability_policy;
pub mod client_tool;
pub mod command_policy;
pub mod config;
pub mod contextual_memory_store;
pub mod conversation_store;
pub mod feature_registry_loader;
pub mod llm;
pub mod llm_call_tracer;
pub mod memory_kind_registry_loader;
pub mod memory_recipe_registry_loader;
pub mod memory_space_resolver;
pub mod memory_subscription_broker;
pub mod rpc_extension;
pub mod shell_exec_approval;
pub mod termination_capability;
mod tool_context;
pub mod tool_registry;
pub mod tool_round_terminator;
pub mod tools;
pub mod turn_events;
pub mod turn_hook;
#[cfg(feature = "memory")]
pub mod work_store;

pub use llm_call_tracer::{LlmCallTracer, NoopLlmCallTracer};

pub use capability_policy::{CapabilityDenied, CapabilityPolicy};
pub use client_tool::{empty_tool_result, ClientToolGate};
pub use command_policy::CommandPolicy;
pub use config::{
    default_conversation_store_root_with_home, validate_external_commands, AppConfig, ConfigError,
    ConfigLoader, ExploreLimitsConfig, ExternalCommandConfig, LlmBackend, LlmGenerationParams,
    LlmProfile, LlmProfilesConfig, LlmProviderKind, MemoryConfig, ProfileRegistry, ReadFileConfig,
    RouterConfig, ShellExecApprovalMode, ShellExecAutoApprovePatterns, ShellExecConfig,
    TerminationStrategy, ToolsConfig, DEFAULT_EXTERNAL_COMMAND_TIMEOUT_SECS,
    DEFAULT_MAX_GREP_FILES_SCANNED, DEFAULT_MAX_GREP_FILE_BYTES, DEFAULT_MAX_GREP_MATCHES,
    DEFAULT_MAX_LIST_ENTRIES, DEFAULT_MAX_TOOL_OUTPUT_BYTES, MIN_MAX_TOOL_ROUNDS,
};
pub use contextual_memory_store::{
    ContextualMemoryStore, ContextualMemoryStoreError, MemoryStoreContext,
};
pub use conversation_store::{
    ConversationIndexEntry, ConversationSnapshot, ConversationStore, ConversationStoreError,
};
pub use feature_registry_loader::FeatureRegistryLoader;
pub use llm::{LlmError, LlmProvider};
pub use memory_kind_registry_loader::MemoryKindRegistryLoader;
pub use memory_recipe_registry_loader::MemoryRecipeRegistryLoader;
pub use memory_space_resolver::MemorySpaceResolver;
pub use memory_subscription_broker::{MemorySubscription, MemorySubscriptionBroker};
pub use rpc_extension::RpcExtension;
pub use shell_exec_approval::ShellExecApprovalGate;
pub use termination_capability::TerminationCapability;
pub use tool_context::ToolExecutionContext;
pub use tool_registry::ToolRegistry;
pub use tool_round_terminator::{TerminationResult, TerminationStrategyUsed, ToolRoundTerminator};
pub use tools::{ToolDefinition, ToolExecutor};
pub use turn_events::{SharedTurnCancellation, TurnCancellation, TurnEventSink};
pub use turn_hook::{TurnHook, TurnHookError};
#[cfg(feature = "memory")]
pub use work_store::{EmptyWorkStore, WorkStore, WorkStoreContext, WorkStoreError};
