//! Outbound ポート: アプリが外界（承認・LLM ストリーム等）を使うための trait

pub mod approval;
pub mod command_allow_rules_loader;
pub mod event_sink_factory;
pub mod llm_event_stream;
pub mod task_runner;

pub use approval::{Approval, ToolApproval};
pub use command_allow_rules_loader::CommandAllowRulesLoader;
pub use event_sink_factory::EventSinkFactory;
pub use llm_event_stream::LlmEventStream;
pub use task_runner::TaskRunner;
