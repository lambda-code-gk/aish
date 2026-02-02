//! 環境変数解決 Outbound ポート
//!
//! セッションディレクトリ・ホームディレクトリを環境変数から解決する。
//! usecase はこの trait 経由でのみ環境変数にアクセスする。

use crate::domain::{HomeDir, SessionDir};
use crate::error::Error;

/// 環境変数解決抽象（Outbound ポート）
///
/// 実装は `common::adapter::StdEnvResolver` やテスト用のモックなど。
pub trait EnvResolver: Send + Sync {
    /// セッションディレクトリを環境変数 AISH_SESSION から取得
    fn session_dir_from_env(&self) -> Option<SessionDir>;

    /// ホームディレクトリを環境変数から解決する
    ///
    /// 優先順位:
    /// 1. AISH_HOME（設定されていれば）
    /// 2. $XDG_CONFIG_HOME/aish（XDG_CONFIG_HOME が設定されていれば）
    /// 3. $HOME/.config/aish
    fn resolve_home_dir(&self) -> Result<HomeDir, Error>;
}
