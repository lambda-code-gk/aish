//! atomic write を提供する FileChangeStore adapter。

use std::path::Path;

use async_trait::async_trait;

use crate::adapters::outbound::tools::file_atomic::atomic_write_file;
use crate::ports::outbound::file_change_store::{FileChangeStore, FileChangeStoreError};

/// filesystem atomic write store。
#[derive(Debug, Clone, Default)]
pub struct FilesystemFileChangeStore;

#[async_trait]
impl FileChangeStore for FilesystemFileChangeStore {
    async fn commit_atomic(
        &self,
        path: &Path,
        content: &[u8],
        preserve_mode: Option<u32>,
    ) -> Result<(), FileChangeStoreError> {
        atomic_write_file(path, content, preserve_mode).map_err(|_| FileChangeStoreError)
    }

    async fn is_regular_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    async fn read_file_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, FileChangeStoreError> {
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|_| FileChangeStoreError)
    }

    async fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    async fn file_byte_len(&self, path: &Path) -> Result<Option<u64>, FileChangeStoreError> {
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::metadata(path)
            .map(|meta| Some(meta.len()))
            .map_err(|_| FileChangeStoreError)
    }
}
