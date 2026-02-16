//! エージェント状態（続き用）の保存・読み込み Outbound ポート
//!
//! 上限到達で停止した会話状態を永続化し、次回 `ai` 実行時に resume するために使う。

use common::domain::SessionDir;
use common::error::Error;
use common::msg::Msg;

/// 会話状態（Vec<Msg>）をセッション dir に保存する能力
pub trait AgentStateSaver: Send + Sync {
    fn save(&self, session_dir: &SessionDir, messages: &[Msg]) -> Result<(), Error>;
    /// 続き用状態を削除する（正常終了時や明示的セッションクリア用。未使用の場合は allow で抑制）
    #[allow(dead_code)]
    fn clear(&self, session_dir: &SessionDir) -> Result<(), Error>;
    /// 続き用メッセージのみ削除し、pending_input は残す（aish のプロンプト注入用）
    fn clear_resume_keep_pending(&self, session_dir: &SessionDir) -> Result<(), Error>;
}

/// 保存された会話状態を読み込む能力
pub trait AgentStateLoader: Send + Sync {
    /// 続き用状態があれば Some、なければ None。ファイルが無い場合も Ok(None)。
    fn load(&self, session_dir: &SessionDir) -> Result<Option<Vec<Msg>>, Error>;
}
