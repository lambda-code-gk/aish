//! 実行時ディレクトリ（XDG / AISH_HOME 解決結果）
//!
//! EnvResolver::resolve_dirs() で取得し、セッション・ログ・キャッシュのパス計算に使う。

use std::path::PathBuf;

/// 解決済みの config / data / state / cache ディレクトリ
#[derive(Debug, Clone)]
pub struct Dirs {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Dirs {
    /// セッション格納ディレクトリ（state/session）
    pub fn sessions_dir(&self) -> PathBuf {
        self.state_dir.join("session")
    }

    /// ログ格納ディレクトリ（state/logs または state 配下の log ファイル用）
    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }
}
