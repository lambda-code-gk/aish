//! 環境変数による設定取得（adapter 層）
//!
//! usecase は環境変数に直接依存せず、adapter 経由で取得する。

use common::domain::SessionDir;
use std::env;
use std::path::PathBuf;

/// セッションディレクトリを環境変数 AISH_SESSION から取得
pub fn session_dir_from_env() -> Option<SessionDir> {
    env::var("AISH_SESSION")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .map(SessionDir::new)
}
