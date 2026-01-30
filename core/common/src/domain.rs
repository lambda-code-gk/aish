//! ドメイン型（Newtype）
//!
//! String / PathBuf を直接運ばず、意味のある型に包んで境界を明確にする。

use std::path::{Path, PathBuf};

/// セッションディレクトリのパス
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDir(PathBuf);

impl SessionDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl std::ops::Deref for SessionDir {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for SessionDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl From<PathBuf> for SessionDir {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

/// ホームディレクトリのパス
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeDir(PathBuf);

impl HomeDir {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl std::ops::Deref for HomeDir {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for HomeDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl From<PathBuf> for HomeDir {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

/// Part ID（8文字 base62、辞書順＝時系列）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartId(String);

impl PartId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for PartId {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for PartId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for PartId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// プロバイダ名（gemini, gpt, echo 等）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderName(String);

impl ProviderName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::ops::Deref for ProviderName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Display for ProviderName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ProviderName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for ProviderName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
