//! ユーザー質問の入力モデル。

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AskInput {
    pub user_message: String,
    pub shell_log_tail: Option<String>,
    /// `ai` プロセスのカレントディレクトリ。aibe の全ツールが相対パス解決に使う（`context.cwd`）。
    pub client_cwd: Option<PathBuf>,
    /// 展開・検証済みツール名（`agent_turn.tools` にそのまま載せる）。
    pub tools: Vec<String>,
}
