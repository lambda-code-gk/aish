//! セッションヒストリー（会話履歴）のドメイン型
//!
//! part ファイルから読み込んだ user/assistant メッセージ列を保持し、
//! LLM の履歴として渡すための不変条件付き型。

use common::llm::provider::Message as LlmMessage;

/// セッションヒストリー（会話のメッセージ列）
#[derive(Debug, Clone)]
pub struct History {
    messages: Vec<LlmMessage>,
}

impl History {
    pub fn new() -> Self {
        History {
            messages: Vec::new(),
        }
    }

    pub fn push_user(&mut self, content: impl Into<String>) {
        self.messages.push(LlmMessage::user(content));
    }

    pub fn push_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(LlmMessage::assistant(content));
    }

    pub fn messages(&self) -> &[LlmMessage] {
        &self.messages
    }

    #[allow(dead_code)] // テストで使用。公開APIとして保持
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
