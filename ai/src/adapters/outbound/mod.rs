mod aibe_client;
mod aibe_config;
mod aibe_external_commands;
mod chat_line_editor;
mod dynamic_completion;
mod file_log;
mod local_history;
mod output_filter;
mod shell_exec_approval_ui;
mod shell_log_resolver;
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
pub use output_filter::{apply_output_filter, format_filter_exit_status, FilterRunOutcome};
pub use shell_exec_approval_ui::prompt_shell_exec_approval;
pub use shell_log_resolver::resolve_shell_log_for_ask;
pub use stdout_presenter::{render_response, StdoutPresenter};
pub use terminal_size::detect_terminal_size;
pub use yes_exec_cache::YesExecCache;
