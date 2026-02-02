//! Inbound ポート: ドライバ（CLI）がアプリを呼び出すインターフェース

use crate::cli::Config;
use common::error::Error;

/// AI アプリケーションを実行する Inbound ポート
///
/// main/cli はこの trait を実装した型（AiUseCase）の run を呼び出す。
pub trait RunAiApp: Send + Sync {
    fn run(&self, config: Config) -> Result<i32, Error>;
}
