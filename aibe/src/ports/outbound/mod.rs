pub mod command_policy;
pub mod config;
pub mod llm;
pub mod shell_exec_approval;
pub mod termination_capability;
mod tool_context;
pub mod tool_registry;
pub mod tool_round_terminator;
pub mod tools;

pub use command_policy::CommandPolicy;
pub use config::{
    AppConfig, ConfigError, ConfigLoader, ExploreLimitsConfig, LlmBackend, LlmGenerationParams,
    LlmProfile, LlmProfilesConfig, LlmProviderKind, ProfileRegistry, ReadFileConfig,
    ShellExecApprovalMode, ShellExecConfig, TerminationStrategy, ToolsConfig,
    DEFAULT_MAX_GREP_FILES_SCANNED, DEFAULT_MAX_GREP_FILE_BYTES, DEFAULT_MAX_GREP_MATCHES,
    DEFAULT_MAX_LIST_ENTRIES, DEFAULT_MAX_TOOL_OUTPUT_BYTES, MIN_MAX_TOOL_ROUNDS,
};
pub use llm::{LlmError, LlmProvider};
pub use shell_exec_approval::ShellExecApprovalGate;
pub use termination_capability::TerminationCapability;
pub use tool_context::ToolExecutionContext;
pub use tool_registry::ToolRegistry;
pub use tool_round_terminator::{TerminationResult, TerminationStrategyUsed, ToolRoundTerminator};
pub use tools::{ToolDefinition, ToolExecutor};
