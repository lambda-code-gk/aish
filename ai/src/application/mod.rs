mod ask;
mod ask_launch;
mod ask_prompt_input;
pub mod client_tools;
mod feature_executor;
mod history;
pub mod memory_cli;
pub mod memory_cli_context;
pub mod memory_cli_pack;
pub mod memory_command_policy;
pub mod memory_space;
#[cfg(not(feature = "memory"))]
pub mod memory_stub;
pub mod replay_manifest;
mod smart_preprocessor;
mod turn_cancel;
pub mod work_cli;

pub use ask::{Ask, AskError, AskOutcome, AskRunOptions};
pub use ask_launch::{ensure_aibe_if_needed, plan_ask_launch, AskLaunchPlan};
pub use ask_prompt_input::{
    classify_from_raw_args, plan_interactive_prompt_route, InteractivePromptRoute,
};
pub use history::{
    build_response_summary, build_summary, current_time_ms, list_history, next_history_id,
    record_turn, HistoryRecordInput, HistoryReplayInput,
};
pub use turn_cancel::{clear_turn_cancel, register_turn_cancel, TurnCancelGuard};

pub use feature_executor::{execute_feature_actions_mvp, FeatureExecutionOutcome};
pub use replay_manifest::ShellLogMode;
pub use smart_preprocessor::{evaluate_preprocessor, PreprocessorRunInput, PreprocessorRunOutcome};
