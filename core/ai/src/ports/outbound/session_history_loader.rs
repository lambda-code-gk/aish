//! セッション履歴読み込みの Outbound ポート
//!
//! セッションディレクトリから履歴（Part ファイル群など）を読み、History を返す。

use crate::domain::History;
use common::domain::SessionDir;
use common::error::Error;

/// セッション履歴を読み込む能力
pub trait SessionHistoryLoader: Send + Sync {
    fn load(&self, session_dir: &SessionDir) -> Result<History, Error>;
}
