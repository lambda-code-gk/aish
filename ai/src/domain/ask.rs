//! ユーザー質問の入力モデル。

#[derive(Debug, Clone)]
pub struct AskInput {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
}
