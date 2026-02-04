//! セッション応答保存の Outbound ポート
//!
//! 1 回分の assistant 応答をセッションに保存する。

use common::domain::SessionDir;
use common::error::Error;

/// セッションに assistant 応答を 1 件保存する能力
pub trait SessionResponseSaver: Send + Sync {
    fn save_assistant(&self, session_dir: &SessionDir, response: &str) -> Result<(), Error>;
}
