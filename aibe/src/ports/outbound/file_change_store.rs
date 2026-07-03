//! ファイル変更 store outbound port（設計 §24.3 — Phase 5 で拡張）。

use std::path::Path;

use async_trait::async_trait;
use thiserror::Error;

/// atomic commit エラー。
#[derive(Debug, Error)]
#[error("write_failed")]
pub struct FileChangeStoreError;

#[async_trait]
pub trait FileChangeStore: Send + Sync {
    async fn commit_atomic(
        &self,
        path: &Path,
        content: &[u8],
        preserve_mode: Option<u32>,
    ) -> Result<(), FileChangeStoreError>;

    /// 対象 path が通常ファイルとして存在するか（revalidate 用）。
    async fn is_regular_file(&self, path: &Path) -> bool;

    /// 対象 path の bytes を読む。不存在なら `Ok(None)`。
    async fn read_file_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, FileChangeStoreError>;

    /// 対象 path が存在するか（create revalidate 用）。
    async fn path_exists(&self, path: &Path) -> bool;
}
