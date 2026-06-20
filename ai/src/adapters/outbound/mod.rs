mod aibe_client;
mod aibe_config;
mod aibe_external_commands;
mod chat_line_editor;
mod dynamic_completion;
mod file_log;
mod local_history;
mod memory_recipe_approval_ui;
mod output_filter;
pub mod project_key;
mod shell_exec_approval_ui;
mod shell_log_resolver;
mod smart_preprocessor_model;
pub mod smart_preprocessor_observation;
mod stderr_spinner;
mod stdout_presenter;
mod terminal_size;
pub mod toml_config;
mod yes_exec_cache;

pub use aibe_client::AibeUnixClient;
pub use aibe_config::{load_shell_exec_approval, AibeShellExecApproval};
pub use aibe_external_commands::external_command_names;
pub use chat_line_editor::{read_chat_line, ChatReadLineResult};
pub use dynamic_completion::{
    complete_preset, complete_profile, complete_session, complete_tools_token, list_profile_names,
    list_session_ids,
};
pub use file_log::FileLogTail;
pub use local_history::LocalHistoryStore;
pub use memory_recipe_approval_ui::{
    parse_memory_recipe_apply_choice, prompt_memory_recipe_apply,
    stdin_ready_for_memory_recipe_apply,
};
pub use output_filter::{apply_output_filter, format_filter_exit_status, FilterRunOutcome};
pub use shell_exec_approval_ui::{
    approval_prompt_stderr_lines, emit_auto_approved_shell_exec,
    escape_for_shell_exec_approval_display, format_auto_approved_shell_exec_line,
    format_shell_exec_invocation, parse_shell_exec_choice, prompt_shell_exec_approval,
};
pub use shell_log_resolver::resolve_shell_log_for_ask;
pub use smart_preprocessor_model::{
    bundled_model_path, load_bundled_preprocessor_model, load_preprocessor_model,
    ValidatedPreprocessorModel,
};
pub use smart_preprocessor_observation::{
    default_observation_path, resolve_session_error_summary, write_observation_record,
    LocalRouteMetrics, ObservationContext, ObservationRecord,
};
pub use stdout_presenter::{
    format_shell_exec_executed_summary, render_response, ShellExecRenderOptions, StdoutPresenter,
};
pub use terminal_size::detect_terminal_size;
pub use yes_exec_cache::YesExecCache;
