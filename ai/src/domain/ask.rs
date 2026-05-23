//! ユーザー質問の入力モデル。

#[derive(Debug, Clone)]
pub struct AskInput {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// 展開・検証済みツール名（`agent_turn.tools` にそのまま載せる）。
    pub tools: Vec<String>,
}
