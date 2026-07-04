//! 0700 / 0600 権限付き filesystem ヘルパ（journal / atomic write 共有）。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn create_dir_0700(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    set_permissions_0700(path)
}

pub(crate) fn set_permissions_0600(path: &Path) -> io::Result<()> {
    set_permissions_mode(path, 0o600)
}

pub(crate) fn set_permissions_0700(path: &Path) -> io::Result<()> {
    set_permissions_mode(path, 0o700)
}

fn set_permissions_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
}

/// 同ディレクトリへ temp を書き、rename で置換する（設計 §18）。
pub(crate) fn atomic_write_in_place(
    path: &Path,
    bytes: &[u8],
    preserve_mode: Option<u32>,
    temp_prefix: &str,
) -> io::Result<()> {
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!("{temp_prefix}{}.{}.tmp", std::process::id(), seq));

    let result = (|| {
        let final_mode = if let Some(mode) = preserve_mode {
            mode & 0o777
        } else {
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o666)
                .open(&temp)?;
            drop(file);
            let mode = fs::metadata(&temp)?.permissions().mode() & 0o777;
            set_permissions_mode(&temp, 0o600)?;
            mode
        };

        let mut file = if preserve_mode.is_some() {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temp)?
        } else {
            OpenOptions::new().write(true).open(&temp)?
        };
        file.write_all(bytes)?;
        file.sync_all()?;
        set_permissions_mode(&temp, final_mode)?;
        fs::rename(&temp, path)?;
        let _ = File::open(parent).and_then(|f| f.sync_all());
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
