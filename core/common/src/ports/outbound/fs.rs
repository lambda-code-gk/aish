//! ファイルシステム Outbound ポート
//!
//! usecase はこの trait 経由でのみファイル I/O を行う。

use crate::error::Error;
use std::path::{Path, PathBuf};

/// ファイルメタデータ（存在・サイズ・種別）
#[derive(Debug, Clone)]
pub struct FileMetadata {
    len: u64,
    is_file: bool,
    is_dir: bool,
}

impl FileMetadata {
    pub fn new(len: u64, is_file: bool, is_dir: bool) -> Self {
        Self { len, is_file, is_dir }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_file(&self) -> bool {
        self.is_file
    }

    pub fn is_dir(&self) -> bool {
        self.is_dir
    }
}

/// ファイルシステム抽象（Outbound ポート）
///
/// 実装は `common::adapter::StdFileSystem` やテスト用のメモリ FS など。
pub trait FileSystem: Send + Sync {
    fn read_to_string(&self, path: &Path) -> Result<String, Error>;
    fn write(&self, path: &Path, contents: &str) -> Result<(), Error>;
    fn rename(&self, from: &Path, to: &Path) -> Result<(), Error>;
    fn create_dir_all(&self, path: &Path) -> Result<(), Error>;
    fn metadata(&self, path: &Path) -> Result<FileMetadata, Error>;
    fn remove_file(&self, path: &Path) -> Result<(), Error>;
    /// ディレクトリを再帰的に削除する（中身ごと）
    fn remove_dir_all(&self, path: &Path) -> Result<(), Error>;
    /// ディレクトリ直下のエントリのフルパス一覧
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>, Error>;
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, Error>;
    /// 追記用に開く（存在しなければ作成）。返した Writer を drop すると閉じる。
    fn open_append(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, Error>;
    /// ファイルを空にする（存在しなければ作成）
    fn truncate_file(&self, path: &Path) -> Result<(), Error>;

    /// パスが存在するか（metadata が取れれば true）
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }
}
