//! 履歴を予算内に縮約する抽象

use super::ContextBudget;
use common::llm::provider::Message as LlmMessage;

/// 履歴を予算内に縮約する
pub trait HistoryReducer: Send + Sync {
    fn reduce(&self, messages: &[LlmMessage], budget: ContextBudget) -> Vec<LlmMessage>;
}
