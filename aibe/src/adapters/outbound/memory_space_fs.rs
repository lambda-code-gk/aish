//! Contextual Memory と Work が共有する memory-space filesystem 境界。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn spaces_root(aibe_root: &Path) -> PathBuf {
    aibe_root.join("memory").join("spaces")
}

pub(crate) fn space_dir(aibe_root: &Path, memory_space_id: &str) -> PathBuf {
    spaces_root(aibe_root).join(memory_space_id)
}

pub(crate) fn ensure_space_layout(aibe_root: &Path, memory_space_id: &str) -> io::Result<()> {
    create_dir_0700(&spaces_root(aibe_root))?;
    create_dir_0700(&space_dir(aibe_root, memory_space_id))
}

pub(crate) fn acquire_space_lock(aibe_root: &Path, memory_space_id: &str) -> io::Result<SpaceLock> {
    ensure_space_layout(aibe_root, memory_space_id)?;
    let path = space_dir(aibe_root, memory_space_id).join(".lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    set_permissions_0600(&path)?;
    SpaceLock::acquire(file)
}

pub(crate) fn atomic_replace_0600(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid state filename"))?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), seq));

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        set_permissions_0600(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        set_permissions_0600(path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) struct SpaceLock(File);

impl SpaceLock {
    fn acquire(file: File) -> io::Result<Self> {
        use std::os::fd::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(file))
    }
}

impl Drop for SpaceLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

pub(crate) fn create_dir_0700(path: &Path) -> io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    set_permissions_0700(path)
}

pub(crate) fn set_permissions_0600(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

fn set_permissions_0700(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}
