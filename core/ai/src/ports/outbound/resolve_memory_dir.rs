//! メモリ用ディレクトリ解決の Outbound ポート
//!
//! プロジェクトの .aish/memory をカレントから遡って探し、無ければグローバル（AISH_HOME/memory）を返す。

use common::error::Error;
use std::path::PathBuf;

/// メモリ用ディレクトリを解決する能力
///
/// 戻り値: (project_dir, global_dir)。project_dir は .aish/memory が見つかったときのみ Some。
pub trait ResolveMemoryDir: Send + Sync {
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error>;
}
