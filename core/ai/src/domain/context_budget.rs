//! コンテキスト予算（メッセージ数・文字数上限）

#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    pub max_messages: usize,
    pub max_chars: usize,
}

impl ContextBudget {
    pub fn legacy() -> Self {
        Self {
            max_messages: 10_000,
            max_chars: 10_000_000,
        }
    }

    pub fn tail_default() -> Self {
        Self {
            max_messages: 40,
            max_chars: 40_000,
        }
    }
}
