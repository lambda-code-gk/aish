//! セッション応答保存の Outbound ポート
//!
//! 1 回分の user クエリや assistant 応答をセッションに保存する。

use common::domain::SessionDir;
use common::error::Error;

/// セッションに user / assistant の応答を保存する能力
pub trait SessionResponseSaver: Send + Sync {
    fn save_assistant(&self, session_dir: &SessionDir, response: &str) -> Result<(), Error>;
    fn save_user(&self, session_dir: &SessionDir, content: &str) -> Result<(), Error>;
}
