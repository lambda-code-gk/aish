//! 永続メモリの一覧・取得 Outbound ポート
//!
//! ai の metadata.json / entries/<id>.json と同じ形式を読む。

use crate::domain::{MemoryEntry, MemoryListEntry};
use common::error::Error;
use std::path::PathBuf;

/// メモリ用ディレクトリ解決と一覧・取得
pub trait MemoryRepository: Send + Sync {
    /// プロジェクト優先・グローバルのメモリディレクトリを解決する
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error>;

    /// メモリ一覧（project + global をマージ、content なし）
    fn list(&self, project_dir: Option<&std::path::Path>, global_dir: &std::path::Path)
        -> Result<Vec<MemoryListEntry>, Error>;

    /// ID で 1 件取得（project 優先、なければ global）
    fn get(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<MemoryEntry, Error>;

    /// ID のメモリを 1 件削除（project 優先で探索し、該当する entries と metadata を更新）
    fn remove(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<(), Error>;
}
