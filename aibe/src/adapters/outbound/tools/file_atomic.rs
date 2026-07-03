//! 同 dir temp + rename による atomic write（設計 §18）。

use std::io;
use std::path::Path;

use thiserror::Error;

use super::super::secure_fs::atomic_write_in_place;

const TEMP_PREFIX: &str = ".aibe-write-";

/// atomic write 失敗（設計 §21 `write_failed` に対応）。
#[derive(Debug, Error)]
#[error("write_failed")]
pub struct AtomicWriteError;

/// 既存ファイルを atomic に置換する。新規作成時は `preserve_mode = None`。
pub fn atomic_write_file(
    path: &Path,
    content: &[u8],
    preserve_mode: Option<u32>,
) -> Result<(), AtomicWriteError> {
    atomic_write_in_place(path, content, preserve_mode, TEMP_PREFIX).map_err(|_| AtomicWriteError)
}

/// rename 直前に失敗を注入する（テスト専用）。
#[doc(hidden)]
pub fn atomic_write_file_fail_before_rename(
    path: &Path,
    content: &[u8],
    preserve_mode: Option<u32>,
) -> Result<(), AtomicWriteError> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::super::secure_fs::set_permissions_0600;

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .ok_or(AtomicWriteError)
        .map_err(|_| AtomicWriteError)?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!("{TEMP_PREFIX}{}.{}.tmp", std::process::id(), seq));

    let mode = preserve_mode.unwrap_or(0o666) & 0o777;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .open(&temp)
        .map_err(|_| AtomicWriteError)?;
    set_permissions_0600(&temp).map_err(|_| AtomicWriteError)?;
    file.write_all(content).map_err(|_| AtomicWriteError)?;
    file.sync_all().map_err(|_| AtomicWriteError)?;
    let _ = fs::remove_file(&temp);
    Err(AtomicWriteError)
}

/// temp ファイル名プレフィックス（テストで残骸検査に使用）。
pub fn temp_file_prefix() -> &'static str {
    TEMP_PREFIX
}

/// ディレクトリ内に atomic write の temp が残っていないか検査する。
pub fn dir_has_temp_leftovers(dir: &Path) -> io::Result<bool> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(TEMP_PREFIX) && name.ends_with(".tmp") {
            return Ok(true);
        }
    }
    Ok(false)
}
