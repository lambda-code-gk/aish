mod ask;
mod ask_launch;
mod ask_prompt_input;
pub mod client_tools;
mod feature_executor;
mod history;
mod human_handoff;
pub mod memory_cli;
pub mod memory_cli_context;
pub mod memory_cli_pack;
pub mod memory_command_policy;
pub mod memory_space;
#[cfg(not(feature = "memory"))]
pub mod memory_stub;
pub mod replay_manifest;
mod smart_preprocessor;
mod suggested_command_recall;
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
pub use human_handoff::{
    handoff_tool_result_message, HumanHandoffError, HumanHandoffRequest, RunSynchronousHumanHandoff,
};

pub use feature_executor::{execute_feature_actions_mvp, FeatureExecutionOutcome};
pub use replay_manifest::ShellLogMode;
pub use smart_preprocessor::{evaluate_preprocessor, PreprocessorRunInput, PreprocessorRunOutcome};
pub use suggested_command_recall::{
    assistant_content_from_response, persist_suggested_commands, recall_next_command,
    recall_prev_command, resolve_recall_gating, RecallGating, RecallGatingInput,
    RecallPersistOutcome, RecallTurnContext,
};
