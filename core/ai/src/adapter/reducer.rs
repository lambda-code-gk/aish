//! 履歴縮約の具象実装（PassThrough / TailWindow）

use crate::domain::{ContextBudget, HistoryReducer};
use common::llm::provider::Message as LlmMessage;

/// そのまま返す（縮約しない）
pub struct PassThroughReducer;

impl HistoryReducer for PassThroughReducer {
    fn reduce(&self, messages: &[LlmMessage], _budget: ContextBudget) -> Vec<LlmMessage> {
        messages.to_vec()
    }
}

/// 末尾から budget 以内に詰める（決定的）
pub struct TailWindowReducer;

impl HistoryReducer for TailWindowReducer {
    fn reduce(&self, messages: &[LlmMessage], budget: ContextBudget) -> Vec<LlmMessage> {
        if messages.is_empty() {
            return vec![];
        }
        if budget.max_messages == 0 {
            return vec![];
        }
        let mut total_chars = 0usize;
        let mut start_index = 0usize;
        let mut kept_count = 0usize;
        for (i, m) in messages.iter().enumerate().rev() {
            // 末尾1件目は必ず保持して、末尾コンテキストの連続性を守る。
            if kept_count == 0 {
                total_chars += m.content.len();
                start_index = i;
                kept_count = 1;
                continue;
            }
            total_chars += m.content.len();
            if (messages.len() - i) > budget.max_messages || total_chars > budget.max_chars {
                start_index = i + 1;
                break;
            }
            start_index = i;
            kept_count += 1;
        }
        messages[start_index..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tail_window_keeps_tail() {
        let reducer = TailWindowReducer;
        let budget = ContextBudget {
            max_messages: 3,
            max_chars: 1000,
        };
        let messages = vec![
            LlmMessage::user("1"),
            LlmMessage::user("2"),
            LlmMessage::user("3"),
            LlmMessage::user("4"),
        ];
        let got = reducer.reduce(&messages, budget);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].content, "2");
        assert_eq!(got[1].content, "3");
        assert_eq!(got[2].content, "4");
    }

    #[test]
    fn test_tail_window_respects_max_messages() {
        let reducer = TailWindowReducer;
        let budget = ContextBudget {
            max_messages: 2,
            max_chars: 10_000,
        };
        let messages = vec![
            LlmMessage::user("a"),
            LlmMessage::user("b"),
            LlmMessage::user("c"),
        ];
        let got = reducer.reduce(&messages, budget);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].content, "b");
        assert_eq!(got[1].content, "c");
    }

    #[test]
    fn test_tail_window_respects_max_chars() {
        let reducer = TailWindowReducer;
        let budget = ContextBudget {
            max_messages: 100,
            max_chars: 5,
        };
        let messages = vec![
            LlmMessage::user("aaa"),
            LlmMessage::user("bb"),
            LlmMessage::user("c"),
        ];
        let got = reducer.reduce(&messages, budget);
        // 末尾から "c"(1) + "bb"(2) = 3 chars まで収まる。"aaa" を足すと 6 > 5 なので含めない
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].content, "bb");
        assert_eq!(got[1].content, "c");
    }

    #[test]
    fn test_tail_window_order_preserved() {
        let reducer = TailWindowReducer;
        let budget = ContextBudget {
            max_messages: 2,
            max_chars: 1000,
        };
        let messages = vec![
            LlmMessage::user("first"),
            LlmMessage::user("second"),
            LlmMessage::user("third"),
        ];
        let got = reducer.reduce(&messages, budget);
        assert_eq!(got[0].content, "second");
        assert_eq!(got[1].content, "third");
    }

    #[test]
    fn test_tail_window_keeps_last_message_even_if_over_char_budget() {
        let reducer = TailWindowReducer;
        let budget = ContextBudget {
            max_messages: 10,
            max_chars: 3,
        };
        let messages = vec![LlmMessage::user("short"), LlmMessage::user("very-long-last")];
        let got = reducer.reduce(&messages, budget);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].content, "very-long-last");
    }

    #[test]
    fn test_pass_through_returns_all() {
        let reducer = PassThroughReducer;
        let budget = ContextBudget {
            max_messages: 1,
            max_chars: 1,
        };
        let messages = vec![
            LlmMessage::user("hello"),
            LlmMessage::user("world"),
        ];
        let got = reducer.reduce(&messages, budget);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].content, "hello");
        assert_eq!(got[1].content, "world");
    }
}
