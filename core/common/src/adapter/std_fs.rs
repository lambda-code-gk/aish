//! 標準ファイルシステム実装（std::fs を委譲）

use super::fs::{FileMetadata, FileSystem};
use crate::error::Error;
use std::path::{Path, PathBuf};

/// 標準ライブラリの fs をそのまま委譲する FileSystem 実装
#[derive(Debug, Clone, Default)]
pub struct StdFileSystem;

impl FileSystem for StdFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<String, Error> {
        std::fs::read_to_string(path).map_err(|e| {
            Error::io_msg(format!("Failed to read '{}': {}", path.display(), e))
        })
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), Error> {
        std::fs::write(path, contents).map_err(|e| {
            Error::io_msg(format!("Failed to write '{}': {}", path.display(), e))
        })
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), Error> {
        std::fs::rename(from, to).map_err(|e| {
            Error::io_msg(format!(
                "Failed to rename '{}' to '{}': {}",
                from.display(),
                to.display(),
                e
            ))
        })
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(path).map_err(|e| {
            Error::io_msg(format!("Failed to create directory '{}': {}", path.display(), e))
        })
    }

    fn metadata(&self, path: &Path) -> Result<FileMetadata, Error> {
        let m = std::fs::metadata(path).map_err(|e| {
            Error::io_msg(format!(
                "Failed to get metadata for '{}': {}",
                path.display(),
                e
            ))
        })?;
        Ok(FileMetadata::new(m.len(), m.is_file(), m.is_dir()))
    }

    fn remove_file(&self, path: &Path) -> Result<(), Error> {
        std::fs::remove_file(path).map_err(|e| {
            Error::io_msg(format!("Failed to remove file '{}': {}", path.display(), e))
        })
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, Error> {
        let entries = std::fs::read_dir(path).map_err(|e| {
            Error::io_msg(format!(
                "Failed to read directory '{}': {}",
                path.display(),
                e
            ))
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                Error::io_msg(format!("Failed to read directory entry: {}", e))
            })?;
            paths.push(entry.path());
        }
        Ok(paths)
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, Error> {
        std::fs::canonicalize(path).map_err(|e| {
            Error::io_msg(format!(
                "Failed to canonicalize '{}': {}",
                path.display(),
                e
            ))
        })
    }

    fn open_append(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, Error> {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(path)
            .map_err(|e| {
                Error::io_msg(format!("Failed to open '{}' for append: {}", path.display(), e))
            })?;
        Ok(Box::new(f))
    }

    fn truncate_file(&self, path: &Path) -> Result<(), Error> {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .map_err(|e| {
                Error::io_msg(format!(
                    "Failed to create/truncate '{}': {}",
                    path.display(),
                    e
                ))
            })?;
        Ok(())
    }
}
