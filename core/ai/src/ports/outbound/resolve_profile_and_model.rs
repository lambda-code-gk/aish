//! 実際に使用するプロファイル名とモデル名を解決する Outbound ポート
//!
//! -p / -m 省略時は profiles.json の default とプロファイル既定モデルに解決する。

use common::domain::{ModelName, ProviderName};
use common::error::Error;

/// 指定または省略時の実際のプロファイル名・モデル名を返す。
/// 返す (profile_name, model_name) は create_stream で使うものと一致する。
pub trait ResolveProfileAndModel: Send + Sync {
    fn resolve(
        &self,
        provider: Option<&ProviderName>,
        model: Option<&ModelName>,
    ) -> Result<(String, String), Error>;
}
