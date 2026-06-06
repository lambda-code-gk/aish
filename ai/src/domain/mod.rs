mod ask;
mod ask_arg_order;
mod history;
mod llm_profile;
mod log_tail;
mod output_filter;
mod output_format;
mod reports;
mod shell_log_resolve;
mod tools;

pub use ask::{AskInput, AskRequest, AskRequestError};
pub use ask_arg_order::{validate_ask_arg_order, AskArgOrderError};
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
pub use reports::{DiagnosticsReport, DryRunReport, FilterMetadata};
pub use shell_log_resolve::{
    resolve_shell_log_for_ask, ShellLogChoice, ShellLogResolveError, AI_ASK_LOG_SESSION,
};
pub use tools::{
    resolve_tools, tokens_from_config_value, AskToolsConfigRaw, ConfigToolsTokens, ResolvedTools,
    ToolAllowlist, ToolsResolveError, ToolsStartupLine,
};
