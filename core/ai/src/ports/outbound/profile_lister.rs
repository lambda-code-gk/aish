//! プロファイル一覧取得 Outbound ポート
//!
//! usecase はこの trait 経由でのみプロファイル一覧を取得する。

use common::error::Error;

/// 現在有効なプロファイル一覧を返す Outbound ポート
pub trait ProfileLister: Send + Sync {
    /// ソート済みプロファイル名リストとデフォルトプロファイル名を返す
    fn list_profiles(&self) -> Result<(Vec<String>, Option<String>), Error>;
}
