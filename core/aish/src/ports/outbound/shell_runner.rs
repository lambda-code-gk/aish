//! 対話シェル起動の Outbound ポート
//!
//! セッションディレクトリとホームディレクトリを渡して対話シェルを起動する。

use common::error::Error;
use std::path::Path;

/// 対話シェルを起動する
pub trait ShellRunner: Send + Sync {
    fn run(&self, session_dir: &Path, home_dir: &Path) -> Result<i32, Error>;
}
