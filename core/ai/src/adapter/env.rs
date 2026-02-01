//! 環境変数による設定取得（adapter 層）
//!
//! usecase は環境変数に直接依存せず、adapter 経由で取得する。

use common::domain::{HomeDir, SessionDir};
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

/// ホームディレクトリを環境変数から解決する
///
/// 優先順位:
/// 1. AISH_HOME（設定されていれば）
/// 2. $XDG_CONFIG_HOME/aish（XDG_CONFIG_HOME が設定されていれば）
/// 3. $HOME/.config/aish
pub fn resolve_home_dir() -> HomeDir {
    HomeDir::new(
        env::var("AISH_HOME")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let mut p = env::var("XDG_CONFIG_HOME")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        let home = env::var("HOME").expect("HOME is not set");
                        PathBuf::from(home).join(".config")
                    });
                p.push("aish");
                p
            }),
    )
}
