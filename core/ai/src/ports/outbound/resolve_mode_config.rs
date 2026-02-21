//! モード設定解決の Outbound ポート

use crate::domain::ModeConfig;
use common::error::Error;

/// モード名から ModeConfig を解決する（config/mode.d/<name>.json を読む）
pub trait ResolveModeConfig: Send + Sync {
    fn resolve(&self, mode_name: &str) -> Result<Option<ModeConfig>, Error>;

    /// 利用可能なモード名一覧（config/mode.d/*.json の stem）。補完用。
    fn list_names(&self) -> Result<Vec<String>, Error>;
}
