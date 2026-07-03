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
}
