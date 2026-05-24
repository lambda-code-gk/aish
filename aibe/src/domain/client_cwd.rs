//! クライアントのカレントディレクトリ（絶対パス必須）。

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClientCwdError {
    #[error("context.cwd is required when tools are enabled")]
    Missing,
    #[error("context.cwd must be a non-empty absolute path")]
    NotAbsolute,
}

/// クライアント（例: `ai ask`）の作業ディレクトリ。相対パス解決の基準。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCwd(PathBuf);

impl ClientCwd {
    /// 絶対パスから構築する。
    pub fn new(path: PathBuf) -> Result<Self, ClientCwdError> {
        if !path.is_absolute() {
            return Err(ClientCwdError::NotAbsolute);
        }
        Ok(Self(path))
    }

    /// プロトコル上の `context.cwd` 文字列からパースする。
    pub fn parse(raw: &str) -> Result<Self, ClientCwdError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ClientCwdError::NotAbsolute);
        }
        Self::new(PathBuf::from(trimmed))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}
