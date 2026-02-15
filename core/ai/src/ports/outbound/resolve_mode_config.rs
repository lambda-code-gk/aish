//! モード設定解決の Outbound ポート

use crate::domain::ModeConfig;
use common::error::Error;

/// モード名から ModeConfig を解決する（config/mode.d/<name>.json を読む）
pub trait ResolveModeConfig: Send + Sync {
    fn resolve(&self, mode_name: &str) -> Result<Option<ModeConfig>, Error>;
}
