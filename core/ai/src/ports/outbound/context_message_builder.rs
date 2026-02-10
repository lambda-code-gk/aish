//! 履歴＋クエリから LLM 用 Vec<Msg> を構築する Outbound ポート
//!
//! 選別/加工アルゴリズムの差し替えは wiring で完結する。

use crate::domain::Query;
use common::llm::provider::Message as LlmMessage;
use common::msg::Msg;

/// クエリを履歴に追加するかどうか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPlacement {
    /// 既に履歴に含まれている（save_user → load 済み）。末尾に追加しない。
    AlreadyInHistory,
    /// 履歴に含まれていない。末尾に追加する。
    AppendAtEnd,
}

/// 履歴とクエリから LLM 用メッセージ列を構築する
pub trait ContextMessageBuilder: Send + Sync {
    fn build(
        &self,
        history: &[LlmMessage],
        query: Option<&Query>,
        system_instruction: Option<&str>,
        query_placement: QueryPlacement,
    ) -> Vec<Msg>;
}
