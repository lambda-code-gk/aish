//! パス解決 Outbound ポート
//!
//! ホームディレクトリ・セッションディレクトリの解決を抽象化する。
//! usecase はこの trait 経由でのみパス解決を行う。

use crate::error::Error;

/// パス解決の入力（CLI の home_dir / session_dir オプション）
#[derive(Debug, Clone, Default)]
pub struct PathResolverInput {
    pub home_dir: Option<String>,
    pub session_dir: Option<String>,
}

/// パス解決抽象（Outbound ポート）
///
/// 実装は `common::adapter::StdPathResolver` やテスト用のモックなど。
pub trait PathResolver: Send + Sync {
    /// ホームディレクトリ（論理的な AISH_HOME）を解決する
    fn resolve_home_dir(&self, input: &PathResolverInput) -> Result<String, Error>;

    /// セッションディレクトリを解決する
    fn resolve_session_dir(
        &self,
        input: &PathResolverInput,
        home_dir: &str,
    ) -> Result<String, Error>;
}
