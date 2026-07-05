mod ask;
mod ask_arg_order;
mod ask_invocation;
pub mod client_tools;
pub mod collaborative_handoff;
mod console_context;
mod console_hint;
mod history;
mod llm_profile;
mod log_tail;
mod output_filter;
mod output_format;
mod progress;
mod prompt_input;
mod reports;
mod request_context;
mod shell_exec_approval;
mod shell_log;
mod smart_observation_report;
pub mod smart_preprocessor;
pub mod suggested_command_recall;
mod terminal_display;
mod terminal_size;
mod tools;
mod work;

pub use ask::{AskInput, AskRequest, AskRequestError};
pub use ask_arg_order::{validate_ask_arg_order, AskArgOrderError};
pub use ask_invocation::{
    classify_ask_invocation, is_known_cli_head, should_enter_interactive_prompt_mode,
    AskInvocationSource,
};
pub use collaborative_handoff::{
    build_candidate_command, cancel_handoff, checkpoint_has_required_fields,
    checkpoint_serialized_field_names, child_goal_close_is_conflict,
    close_child_goal_on_control_returned, deterministic_candidate_id, finalize_running_tools,
    hash_handoff_token, is_valid_handoff_id, mark_child_goal_closed, mark_running_tools_unknown,
    mark_tool_completed, record_tool_running, should_close_child_goal,
    sync_tool_executions_from_executed, try_transition, upsert_tool_execution, validate_handoff_id,
    validate_shell_token, verify_handoff_token, CancelHandoffError, ChildGoalAchievement,
    ChildGoalCloseReason, ChildGoalCloseState, ChildGoalMeta, CollaborativeAgentRole,
    CollaborativeAuditKind, CollaborativePolicy, CommandCandidate, CommandCandidateSource, Handoff,
    HandoffCheckpoint, HandoffEvent, HandoffLease, HandoffShellSession, HandoffState,
    HandoffTransitionError, InvalidHandoffIdError, RecoverableToolExecution, RecoverableToolStatus,
    RequestHumanAction, RequestedShellExec, CHECKPOINT_REQUIRED_FIELD_NAMES,
    HANDOFF_SCHEMA_VERSION,
};
pub use console_hint::{
    resolve_console_hints, ConsoleHintOutputFormat, ConsoleHintReport, ConsoleHintSource,
    ConsoleHintSuppressedBy,
};
pub use history::{
    HistoryIndexEntry, HistoryIndexFilter, HistoryIndexView, HistoryMessage, HistoryPayload,
    HistoryRecordKind, HistoryRecordStatus, HistoryReplayRequest, HistorySummary, HistoryTurnInput,
};
pub use llm_profile::resolve_llm_profile;
pub use log_tail::{resolve_log_tail_bytes, LogTailResolveError, DEFAULT_LOG_TAIL_BYTES};
pub use output_filter::resolve_output_filter;
pub use output_format::{
    append_env_line, append_tsv_row, shell_single_quote, OutputFormat, OutputFormatError,
};
pub use progress::{format_progress_label, resolve_progress};
pub use prompt_input::{
    is_substantive_prompt, strip_prompt_template_comments, PromptAcquisitionResult,
};
pub use reports::{CollaborativeHandoffReport, DiagnosticsReport, DryRunReport, FilterMetadata};
pub use request_context::RequestContextInput;
pub use shell_exec_approval::{
    canonical_shell_exec_invocation, classify_shell_exec_tier, command_shell_exec_key,
    exact_shell_exec_key, match_shell_exec_auto_approve_pattern,
    parse_shell_exec_auto_approve_patterns, shell_exec_approval_origin_for_choice,
    ShellExecApprovalChoice, ShellExecApprovalDecision, ShellExecAutoApprovePatternSet,
    ShellExecRememberScope, ShellExecSessionState, ShellExecTier,
};
pub use shell_log::{
    validate_session_id, ShellLogChoice, ShellLogResolveError, AI_ASK_LOG_SESSION,
};
pub use smart_observation_report::{
    filter_observations, render_markdown_report, render_recent, SmartObservationLine,
    SmartObservationStats, SmartReportOptions,
};
pub use smart_preprocessor::{
    run_preprocessor, should_short_circuit, PreprocessConfig, PreprocessInput, SmartConfidenceGate,
    SmartIntentClass, SmartPreprocessDecision, SmartPreprocessMode, SmartRouteTurnHints,
    FEATURE_EXTRACTOR_VERSION,
};
pub use suggested_command_recall::{
    extract_shell_candidates_from_content, SuggestedCommandCache, SuggestedCommandCandidate,
    SuggestedCommandQueue, SUGGESTED_COMMAND_MAX_BYTES,
};
pub use terminal_size::TerminalSize;
pub use tools::{
    resolve_tools, tokens_from_config_value, AskToolsConfigRaw, ConfigToolsTokens, ResolvedTools,
    ToolAllowlist, ToolsResolveError, ToolsStartupLine,
};
pub use work::{render_work_snapshot, WorkView};
