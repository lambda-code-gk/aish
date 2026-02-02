//! Outbound ポート: アプリが外界（承認・LLM ストリーム等）を使うための trait

pub mod approval;
pub mod llm_event_stream;

pub use approval::{Approval, ToolApproval};
pub use llm_event_stream::LlmEventStream;
