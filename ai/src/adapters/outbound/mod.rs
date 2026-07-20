mod aibe_client;
mod aibe_config;
mod aibe_external_commands;
mod chat_line_editor;
mod dynamic_completion;
mod external_editor;
mod file_log;
mod file_write_approval_ui;
mod human_handoff;
mod human_task_file_store;
mod local_history;
mod memory_recipe_approval_ui;
mod output_filter;
pub mod project_key;
mod prompt_comment_filter;
mod replay_source;
pub mod shell_completion;
mod shell_exec_approval_ui;
mod shell_log_resolver;
mod smart_observation_log_reader;
mod smart_preprocessor_model;
pub mod smart_preprocessor_observation;
mod smart_preprocessor_trace;
mod stderr_spinner;
mod stdout_presenter;
mod suggested_command_recall_store;
mod terminal_size;
pub mod toml_config;
mod turn_cancel;
mod yes_exec_cache;

pub use ::aibe_client::ToolApprovalDecision;
pub use aibe_client::AibeUnixClient;
pub use aibe_config::{load_shell_exec_approval, AibeShellExecApproval};
pub use aibe_external_commands::external_command_names;
pub use chat_line_editor::{read_chat_line, ChatReadLineResult};
pub use dynamic_completion::{
    complete_preset, complete_profile, complete_session, complete_tools_token, list_profile_names,
    list_session_ids,
};
pub use external_editor::{
    acquire_prompt_via_external_editor, create_prompt_temp_file, resolve_editor_command_from_env,
};
pub use file_log::FileLogTail;
pub use file_write_approval_ui::{
    approval_prompt_stderr_lines as file_write_approval_prompt_stderr_lines,
    escape_for_file_write_approval_display, file_write_approval_decision_from_input,
    parse_file_write_approval_choice, prompt_file_write_approval,
    stdin_ready_for_file_write_approval,
};
pub use human_handoff::{
    allocate_runtime_handoff_path, cleanup_runtime_handoff_dir, create_runtime_handoff_dir,
    AishHumanShellLauncher, ParentTermiosGuard, ProcessEnvironmentObserver, RuntimeHandoffDirGuard,
};
pub use human_task_file_store::{
    HumanTaskFileStore, SystemHumanTaskIdentity, SystemHumanTaskTimeFormatter,
};
pub use local_history::LocalHistoryStore;
pub use memory_recipe_approval_ui::{
    parse_memory_recipe_apply_choice, prompt_memory_recipe_apply,
    stdin_ready_for_memory_recipe_apply,
};
pub use output_filter::{apply_output_filter, format_filter_exit_status, FilterRunOutcome};
pub use replay_source::{
    load_replay_events, load_replay_events_in_range, load_replay_events_in_range_from_file,
    RangedReplayEvents, ReplaySourceError, MAX_EVIDENCE_SCAN_BYTES,
};
pub use shell_completion::{
    recall_env_snippet_for_shell, recall_hook_for_shell, BASH_RECALL_ENV_SNIPPET, BASH_RECALL_HOOK,
    ZSH_RECALL_ENV_SNIPPET, ZSH_RECALL_HOOK,
};
pub use shell_exec_approval_ui::{
    approval_prompt_stderr_lines, emit_auto_approved_shell_exec,
    escape_for_shell_exec_approval_display, format_auto_approved_shell_exec_line,
    format_shell_exec_invocation, parse_shell_exec_choice, prompt_shell_exec_approval,
};
pub use shell_log_resolver::resolve_shell_log_for_ask;
pub use smart_observation_log_reader::{
    expand_observation_path, read_smart_observation_log, SmartObservationRead,
    SmartObservationReadError,
};
pub use smart_preprocessor_model::{
    bundled_model_path, load_bundled_preprocessor_model, load_preprocessor_model,
    ValidatedPreprocessorModel,
};
pub use smart_preprocessor_observation::{
    default_observation_path, finalize_preprocessor_observation, resolve_session_error_summary,
    write_observation_record, LocalRouteMetrics, ObservationContext, ObservationRecord,
    PreprocessorObservationDraft, TurnLlmAccounting,
};
pub use smart_preprocessor_trace::smart_preprocessor_trace_enabled;
pub use stdout_presenter::{
    format_shell_exec_executed_summary, format_tool_call_line, render_response,
    render_response_structured, ShellExecRenderOptions, StdoutPresenter,
};
pub use suggested_command_recall_store::{
    default_suggestion_cache_path, resolve_suggestion_cache_path, FileSuggestedCommandRecallStore,
};
pub use terminal_size::detect_terminal_size;
pub use turn_cancel::{
    clear_turn_cancel, register_turn_cancel, signal_cancel_requested, TurnCancelGuard,
};
pub use yes_exec_cache::YesExecCache;
