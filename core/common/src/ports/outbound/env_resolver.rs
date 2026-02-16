//! 環境変数解決 Outbound ポート
//!
//! セッションディレクトリ・ホームディレクトリを環境変数から解決する。
//! usecase はこの trait 経由でのみ環境変数にアクセスする。

use crate::domain::{Dirs, HomeDir, SessionDir};
use crate::error::Error;
use std::path::PathBuf;

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

    /// config / data / state / cache を一括解決する
    ///
    /// AISH_HOME があればその配下の config, data, state, cache。
    /// なければ XDG に従い $XDG_CONFIG_HOME/aish, $XDG_DATA_HOME/aish, $XDG_STATE_HOME/aish, $XDG_CACHE_HOME/aish。
    fn resolve_dirs(&self) -> Result<Dirs, Error>;

    /// カレントディレクトリを返す（プロジェクトスコープ探索用）
    fn current_dir(&self) -> Result<PathBuf, Error>;

    /// グローバル system.d ディレクトリ
    /// AISH_HOME が設定されていれば $AISH_HOME/config/system.d、そうでなければ ~/.config/aish/system.d
    fn resolve_global_system_d_dir(&self) -> Result<Option<PathBuf>, Error>;

    /// ユーザー system.d ディレクトリ（~/.aish/system.d）
    fn resolve_user_system_d_dir(&self) -> Result<Option<PathBuf>, Error>;

    /// プロバイダプロファイル設定ファイルのパス
    /// AISH_HOME があれば $AISH_HOME/config/profiles.json、なければ resolve_home_dir() 直下の profiles.json（例: ~/.config/aish/profiles.json）
    fn resolve_profiles_config_path(&self) -> Result<PathBuf, Error>;

    /// ログファイルのパス（JSONL 出力先）
    /// AISH_HOME が設定されていれば $AISH_HOME/state/log.jsonl。
    /// 未設定なら $XDG_STATE_HOME/aish/log.jsonl（XDG_STATE_HOME 未設定時は ~/.local/state/aish/log.jsonl）。
    fn resolve_log_file_path(&self) -> Result<PathBuf, Error>;
}
