//! Inbound ポート: ドライバ（CLI）がアプリを呼び出すインターフェース

use crate::cli::Config;
use common::error::Error;

/// ユースケースを実行する Inbound ポート（Command ディスパッチの入口）
///
/// main/cli はこの trait を実装した型（App 等）の run を呼び出す。
pub trait UseCaseRunner: Send + Sync {
    fn run(&self, config: Config) -> Result<i32, Error>;
}
