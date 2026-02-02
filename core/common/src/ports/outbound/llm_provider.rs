//! LLM プロバイダ Outbound ポート（re-export）
//!
//! トレイト定義は llm/provider にあり、循環参照を避けるためここで re-export する。

pub use crate::llm::provider::LlmProvider;
