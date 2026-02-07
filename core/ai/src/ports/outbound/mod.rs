//! Outbound ポート: アプリが外界（承認・LLM ストリーム等）を使うための trait

pub mod agent_state_storage;
pub mod approval;
pub mod command_allow_rules_loader;
pub mod continue_prompt;
pub mod event_sink_factory;
pub mod interrupt_checker;
pub mod llm_event_stream;
pub mod llm_event_stream_factory;
pub mod profile_lister;
pub mod resolve_system_instruction;
pub mod run_query;
pub mod session_history_loader;
pub mod session_response_saver;
pub mod task_runner;

pub use agent_state_storage::{AgentStateLoader, AgentStateSaver};
pub use approval::{Approval, ToolApproval};
pub use command_allow_rules_loader::CommandAllowRulesLoader;
pub use continue_prompt::ContinueAfterLimitPrompt;
pub use event_sink_factory::EventSinkFactory;
pub use interrupt_checker::InterruptChecker;
pub use llm_event_stream::LlmEventStream;
pub use llm_event_stream_factory::{LlmEventStreamFactory, LlmStreamContext};
pub use profile_lister::ProfileLister;
pub use resolve_system_instruction::ResolveSystemInstruction;
pub use run_query::RunQuery;
pub use session_history_loader::SessionHistoryLoader;
pub use session_response_saver::SessionResponseSaver;
pub use task_runner::TaskRunner;
