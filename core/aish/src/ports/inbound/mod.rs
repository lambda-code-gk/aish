//! Inbound ポート: ドライバ（CLI）がアプリを呼び出すインターフェース

use crate::cli::Config;
use common::error::Error;

/// Aish アプリケーションを実行する Inbound ポート
///
/// main/cli はこの trait を実装した型（AishUseCase）の run を呼び出す。
#[cfg(unix)]
pub trait RunAishApp: Send + Sync {
    fn run(&self, config: Config) -> Result<i32, Error>;
}
