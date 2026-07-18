use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::domain::human_task_checkpoint::{
    HumanTaskCheckpointV1, HumanTaskId, HUMAN_TASK_CHECKPOINT_MAX_BYTES,
};
use crate::ports::outbound::{
    HumanTaskIdentity, HumanTaskStore, HumanTaskStoreError, HumanTaskStoreLock,
    HumanTaskTimeFormatter,
};

pub struct HumanTaskFileStore {
    root: PathBuf,
}

pub struct SystemHumanTaskIdentity;
pub struct SystemHumanTaskTimeFormatter;

struct HumanTaskFileLock(File);

impl HumanTaskStoreLock for HumanTaskFileLock {}

impl Drop for HumanTaskFileLock {
    fn drop(&mut self) {
        let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl HumanTaskTimeFormatter for SystemHumanTaskTimeFormatter {
    fn format_local(&self, timestamp_ms: u64) -> String {
        let seconds = (timestamp_ms / 1000).min(libc::time_t::MAX as u64) as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&seconds, &mut local) }.is_null() {
            return format!("{timestamp_ms} ms since Unix epoch");
        }
        let mut buffer = [0i8; 40];
        let format = b"%Y-%m-%d %H:%M:%S %z\0";
        let written = unsafe {
            libc::strftime(
                buffer.as_mut_ptr(),
                buffer.len(),
                format.as_ptr().cast(),
                &local,
            )
        };
        if written == 0 {
            return format!("{timestamp_ms} ms since Unix epoch");
        }
        let bytes = buffer[..written]
            .iter()
            .map(|value| *value as u8)
            .collect::<Vec<_>>();
        String::from_utf8(bytes).unwrap_or_else(|_| format!("{timestamp_ms} ms since Unix epoch"))
    }
}
impl HumanTaskIdentity for SystemHumanTaskIdentity {
    fn new_task_id(&self) -> HumanTaskId {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let days = now.as_secs() / 86_400;
        let (year, month, day) = civil_from_days(days as i64);
        let mut random = [0u8; 3];
        if File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .is_err()
        {
            random.copy_from_slice(&(now.subsec_nanos() & 0x00ff_ffff).to_be_bytes()[1..]);
        }
        HumanTaskId::parse(format!(
            "ht-{year:04}{month:02}{day:02}-{:02x}{:02x}{:02x}",
            random[0], random[1], random[2]
        ))
        .unwrap_or_else(|_| unreachable!("generated task id has a fixed valid shape"))
    }
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    (y, m, d)
}

impl HumanTaskFileStore {
    pub fn new(history_dir: PathBuf) -> Self {
        Self {
            root: history_dir.join("human-tasks"),
        }
    }

    fn ensure_dir(path: &Path) -> Result<(), HumanTaskStoreError> {
        if path.exists() {
            let md = fs::symlink_metadata(path).map_err(|_| HumanTaskStoreError::Unavailable)?;
            if md.file_type().is_symlink() || !md.is_dir() {
                return Err(HumanTaskStoreError::PermissionDenied);
            }
            if md.uid() != unsafe { libc::geteuid() } || md.permissions().mode() & 0o7777 != 0o700 {
                return Err(HumanTaskStoreError::PermissionDenied);
            }
            return Ok(());
        }
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
            .create(path)
            .map_err(|_| HumanTaskStoreError::Unavailable)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| HumanTaskStoreError::Unavailable)?;
        Self::ensure_dir(path)
    }

    fn checkpoint_path(&self, id: &HumanTaskId) -> Result<PathBuf, HumanTaskStoreError> {
        HumanTaskId::parse(id.as_str()).map_err(|_| HumanTaskStoreError::Invalid)?;
        Ok(self.root.join(id.as_str()).join("checkpoint.json"))
    }

    fn read_checkpoint(path: &Path) -> Result<HumanTaskCheckpointV1, HumanTaskStoreError> {
        let md = fs::symlink_metadata(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                HumanTaskStoreError::NotFound
            } else {
                HumanTaskStoreError::Unavailable
            }
        })?;
        if md.file_type().is_symlink()
            || !md.is_file()
            || md.uid() != unsafe { libc::geteuid() }
            || md.permissions().mode() & 0o7777 != 0o600
        {
            return Err(HumanTaskStoreError::PermissionDenied);
        }
        if md.len() as usize > HUMAN_TASK_CHECKPOINT_MAX_BYTES {
            return Err(HumanTaskStoreError::Invalid);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|_| HumanTaskStoreError::PermissionDenied)?;
        let mut bytes = Vec::with_capacity(md.len() as usize);
        file.take(HUMAN_TASK_CHECKPOINT_MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| HumanTaskStoreError::Unavailable)?;
        if bytes.len() > HUMAN_TASK_CHECKPOINT_MAX_BYTES {
            return Err(HumanTaskStoreError::Invalid);
        }
        let raw: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| HumanTaskStoreError::Invalid)?;
        if raw.get("version").and_then(|v| v.as_u64()) != Some(1) {
            return Err(HumanTaskStoreError::VersionUnsupported);
        }
        let checkpoint: HumanTaskCheckpointV1 =
            serde_json::from_value(raw).map_err(|_| HumanTaskStoreError::Invalid)?;
        checkpoint
            .validate()
            .map_err(|_| HumanTaskStoreError::Invalid)?;
        Ok(checkpoint)
    }

    fn open_root_lock_file(&self) -> Result<File, HumanTaskStoreError> {
        if let Some(parent) = self.root.parent() {
            fs::create_dir_all(parent).map_err(|_| HumanTaskStoreError::Unavailable)?;
        }
        Self::ensure_dir(&self.root)?;
        let path = self.root.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|_| HumanTaskStoreError::PermissionDenied)?;
        let metadata = file
            .metadata()
            .map_err(|_| HumanTaskStoreError::Unavailable)?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o7777 != 0o600
        {
            return Err(HumanTaskStoreError::PermissionDenied);
        }
        Ok(file)
    }
}

impl HumanTaskStore for HumanTaskFileStore {
    fn lock_exclusive(&self) -> Result<Box<dyn HumanTaskStoreLock + '_>, HumanTaskStoreError> {
        let file = self.open_root_lock_file()?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(HumanTaskStoreError::Unavailable);
        }
        Ok(Box::new(HumanTaskFileLock(file)))
    }

    fn try_lock_exclusive(
        &self,
    ) -> Result<Option<Box<dyn HumanTaskStoreLock + '_>>, HumanTaskStoreError> {
        let file = self.open_root_lock_file()?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Box::new(HumanTaskFileLock(file))));
        }
        let errno = std::io::Error::last_os_error().raw_os_error();
        if errno == Some(libc::EWOULDBLOCK) || errno == Some(libc::EAGAIN) {
            return Ok(None);
        }
        Err(HumanTaskStoreError::Unavailable)
    }

    fn load_active(&self) -> Result<HumanTaskCheckpointV1, HumanTaskStoreError> {
        if !self.root.exists() {
            return Err(HumanTaskStoreError::NotFound);
        }
        Self::ensure_dir(&self.root)?;
        let mut found = None;
        for entry in fs::read_dir(&self.root).map_err(|_| HumanTaskStoreError::Unavailable)? {
            let entry = entry.map_err(|_| HumanTaskStoreError::Unavailable)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| HumanTaskStoreError::Invalid)?;
            if name == "lock" || name == ".removing" {
                continue;
            }
            let id = HumanTaskId::parse(name).map_err(|_| HumanTaskStoreError::Invalid)?;
            let dir = entry.path();
            Self::ensure_dir(&dir)?;
            let checkpoint = match Self::read_checkpoint(&dir.join("checkpoint.json")) {
                Err(HumanTaskStoreError::NotFound) => return Err(HumanTaskStoreError::Invalid),
                other => other?,
            };
            if checkpoint.task_id != id || found.is_some() {
                return Err(HumanTaskStoreError::Invalid);
            }
            found = Some(checkpoint);
        }
        found.ok_or(HumanTaskStoreError::NotFound)
    }

    fn save(&self, checkpoint: &HumanTaskCheckpointV1) -> Result<(), HumanTaskStoreError> {
        checkpoint.validate().map_err(|e| {
            if e.contains("version") {
                HumanTaskStoreError::VersionUnsupported
            } else {
                HumanTaskStoreError::Invalid
            }
        })?;
        let bytes = serde_json::to_vec(checkpoint).map_err(|_| HumanTaskStoreError::Invalid)?;
        if bytes.len() > HUMAN_TASK_CHECKPOINT_MAX_BYTES {
            return Err(HumanTaskStoreError::Invalid);
        }
        if let Some(parent) = self.root.parent() {
            fs::create_dir_all(parent).map_err(|_| HumanTaskStoreError::Unavailable)?;
        }
        Self::ensure_dir(&self.root)?;
        let path = self.checkpoint_path(&checkpoint.task_id)?;
        let dir = path.parent().ok_or(HumanTaskStoreError::Unavailable)?;
        Self::ensure_dir(dir)?;
        let temp = dir.join(format!(".checkpoint.{}.tmp", std::process::id()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp)
                .map_err(|_| HumanTaskStoreError::Unavailable)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| HumanTaskStoreError::Unavailable)?;
            file.write_all(&bytes)
                .map_err(|_| HumanTaskStoreError::Unavailable)?;
            file.sync_all()
                .map_err(|_| HumanTaskStoreError::Unavailable)?;
            fs::rename(&temp, &path).map_err(|_| HumanTaskStoreError::Unavailable)?;
            File::open(dir)
                .and_then(|f| f.sync_all())
                .map_err(|_| HumanTaskStoreError::Unavailable)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn remove(&self, task_id: &HumanTaskId) -> Result<(), HumanTaskStoreError> {
        let path = self.checkpoint_path(task_id)?;
        Self::ensure_dir(&self.root)?;
        Self::ensure_dir(path.parent().ok_or(HumanTaskStoreError::Unavailable)?)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                HumanTaskStoreError::NotFound
            } else {
                HumanTaskStoreError::Unavailable
            }
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o7777 != 0o600
        {
            return Err(HumanTaskStoreError::PermissionDenied);
        }
        fs::remove_file(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                HumanTaskStoreError::NotFound
            } else {
                HumanTaskStoreError::Unavailable
            }
        })?;
        fs::remove_dir(path.parent().ok_or(HumanTaskStoreError::Unavailable)?)
            .map_err(|_| HumanTaskStoreError::Unavailable)
    }

    fn remove_invalid_active(&self) -> Result<String, HumanTaskStoreError> {
        Self::ensure_dir(&self.root)?;
        remove_invalid_active_pinned(&self.root)
    }
}

/// Force-clean a single invalid task residue under `root` without following symlinks.
///
/// Pins the checkpoint root with `O_DIRECTORY|O_NOFOLLOW` and uses that fd for listing,
/// `openat`, and `renameat`. Validates every child before any quarantine/delete so nested
/// or foreign entries fail closed without changing the visible residue path.
fn remove_invalid_active_pinned(root: &Path) -> Result<String, HumanTaskStoreError> {
    use std::ffi::CString;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;

    let root_c =
        CString::new(root.as_os_str().as_bytes()).map_err(|_| HumanTaskStoreError::Invalid)?;
    let root_fd = unsafe {
        libc::open(
            root_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if root_fd < 0 {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    let root_owned = unsafe { OwnedFd::from_raw_fd(root_fd) };
    let euid = unsafe { libc::geteuid() };
    let mut root_st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(root_owned.as_raw_fd(), &mut root_st) } != 0
        || root_st.st_uid != euid
        || (root_st.st_mode & 0o7777) != 0o700
    {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    after_root_pin_hook(root);

    let name = list_single_task_residue(root_owned.as_raw_fd())?;
    let name_c = CString::new(name.as_str()).map_err(|_| HumanTaskStoreError::Invalid)?;

    let path_fd = unsafe {
        libc::openat(
            root_owned.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if path_fd < 0 {
        return Err(HumanTaskStoreError::Unavailable);
    }
    let path_owned = unsafe { OwnedFd::from_raw_fd(path_fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(path_owned.as_raw_fd(), &mut st) } != 0 {
        return Err(HumanTaskStoreError::Unavailable);
    }
    if st.st_uid != euid {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    let file_type = st.st_mode & libc::S_IFMT;
    if file_type == libc::S_IFLNK {
        if unsafe { libc::unlinkat(root_owned.as_raw_fd(), name_c.as_ptr(), 0) } != 0 {
            return Err(HumanTaskStoreError::Unavailable);
        }
        return Ok(name);
    }
    if file_type != libc::S_IFDIR {
        return Err(HumanTaskStoreError::PermissionDenied);
    }

    let original_mode = st.st_mode & 0o7777;
    let mut mode_changed = false;
    if original_mode != 0o700 {
        chmod_via_proc(path_owned.as_raw_fd(), 0o700)?;
        mode_changed = true;
    }

    let dot = CString::new(".").map_err(|_| HumanTaskStoreError::Invalid)?;
    let dir_fd = unsafe {
        libc::openat(
            path_owned.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if dir_fd < 0 {
        if mode_changed {
            let _ = chmod_via_proc(path_owned.as_raw_fd(), original_mode);
        }
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    let dir_owned = unsafe { OwnedFd::from_raw_fd(dir_fd) };

    let children = match list_removable_children(dir_owned.as_raw_fd(), euid) {
        Ok(children) => children,
        Err(error) => {
            if mode_changed {
                let _ =
                    unsafe { libc::fchmod(dir_owned.as_raw_fd(), original_mode as libc::mode_t) };
            }
            return Err(error);
        }
    };

    let removing_owned = open_or_create_removing_dir(root_owned.as_raw_fd(), euid)?;
    let quarantine = format!("{}.{}", std::process::id(), name);
    let quarantine_c =
        CString::new(quarantine.as_str()).map_err(|_| HumanTaskStoreError::Invalid)?;
    if unsafe {
        libc::renameat(
            root_owned.as_raw_fd(),
            name_c.as_ptr(),
            removing_owned.as_raw_fd(),
            quarantine_c.as_ptr(),
        )
    } != 0
    {
        if mode_changed {
            let _ = unsafe { libc::fchmod(dir_owned.as_raw_fd(), original_mode as libc::mode_t) };
        }
        return Err(HumanTaskStoreError::Unavailable);
    }
    after_quarantine_hook(root, &quarantine);
    after_directory_pin_hook(root, &quarantine);

    for child in &children {
        let child_c = CString::new(child.as_str()).map_err(|_| HumanTaskStoreError::Invalid)?;
        if unsafe { libc::unlinkat(dir_owned.as_raw_fd(), child_c.as_ptr(), 0) } != 0 {
            let _ = unsafe {
                libc::renameat(
                    removing_owned.as_raw_fd(),
                    quarantine_c.as_ptr(),
                    root_owned.as_raw_fd(),
                    name_c.as_ptr(),
                )
            };
            if mode_changed {
                let _ =
                    unsafe { libc::fchmod(dir_owned.as_raw_fd(), original_mode as libc::mode_t) };
            }
            return Err(HumanTaskStoreError::Unavailable);
        }
    }

    // Empty quarantine directory may remain; shared-path AT_REMOVEDIR is intentionally avoided.
    Ok(name)
}

fn chmod_via_proc(fd: libc::c_int, mode: u32) -> Result<(), HumanTaskStoreError> {
    use std::ffi::CString;
    let proc_path =
        CString::new(format!("/proc/self/fd/{fd}")).map_err(|_| HumanTaskStoreError::Invalid)?;
    if unsafe { libc::chmod(proc_path.as_ptr(), mode as libc::mode_t) } != 0 {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    Ok(())
}

fn open_or_create_removing_dir(
    root_fd: libc::c_int,
    euid: libc::uid_t,
) -> Result<std::os::fd::OwnedFd, HumanTaskStoreError> {
    use std::ffi::CString;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::io::AsRawFd;

    let removing_c = CString::new(".removing").map_err(|_| HumanTaskStoreError::Invalid)?;
    let open_removing = |flags| unsafe { libc::openat(root_fd, removing_c.as_ptr(), flags) };
    let removing_fd =
        open_removing(libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    if removing_fd >= 0 {
        let owned = unsafe { OwnedFd::from_raw_fd(removing_fd) };
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(owned.as_raw_fd(), &mut st) } != 0
            || st.st_uid != euid
            || (st.st_mode & 0o7777) != 0o700
        {
            return Err(HumanTaskStoreError::PermissionDenied);
        }
        return Ok(owned);
    }
    if unsafe { libc::mkdirat(root_fd, removing_c.as_ptr(), 0o700) } != 0
        && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
    {
        return Err(HumanTaskStoreError::Unavailable);
    }
    let fd = open_removing(libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    if fd < 0 {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(owned.as_raw_fd(), &mut st) } != 0
        || st.st_uid != euid
        || (st.st_mode & libc::S_IFMT) != libc::S_IFDIR
        || (st.st_mode & 0o7777) != 0o700
    {
        return Err(HumanTaskStoreError::PermissionDenied);
    }
    Ok(owned)
}

fn list_single_task_residue(root_fd: libc::c_int) -> Result<String, HumanTaskStoreError> {
    let list_fd = unsafe { libc::dup(root_fd) };
    if list_fd < 0 {
        return Err(HumanTaskStoreError::Unavailable);
    }
    let dirp = unsafe { libc::fdopendir(list_fd) };
    if dirp.is_null() {
        unsafe {
            libc::close(list_fd);
        }
        return Err(HumanTaskStoreError::Unavailable);
    }
    let mut residue = None;
    loop {
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(dirp) };
        if entry.is_null() {
            let err = unsafe { *libc::__errno_location() };
            unsafe {
                libc::closedir(dirp);
            }
            if err != 0 {
                return Err(HumanTaskStoreError::Unavailable);
            }
            break;
        }
        let d_name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let name = match d_name.to_str() {
            Ok("." | ".." | "lock" | ".removing") => continue,
            Ok(value) => value.to_string(),
            Err(_) => {
                unsafe {
                    libc::closedir(dirp);
                }
                return Err(HumanTaskStoreError::Invalid);
            }
        };
        if HumanTaskId::parse(&name).is_err() {
            unsafe {
                libc::closedir(dirp);
            }
            return Err(HumanTaskStoreError::Invalid);
        }
        if residue.replace(name).is_some() {
            unsafe {
                libc::closedir(dirp);
            }
            return Err(HumanTaskStoreError::Invalid);
        }
    }
    residue.ok_or(HumanTaskStoreError::NotFound)
}

fn list_removable_children(
    dir_fd: libc::c_int,
    euid: libc::uid_t,
) -> Result<Vec<String>, HumanTaskStoreError> {
    use std::ffi::CString;

    let list_fd = unsafe { libc::dup(dir_fd) };
    if list_fd < 0 {
        return Err(HumanTaskStoreError::Unavailable);
    }
    let dirp = unsafe { libc::fdopendir(list_fd) };
    if dirp.is_null() {
        unsafe {
            libc::close(list_fd);
        }
        return Err(HumanTaskStoreError::Unavailable);
    }
    let mut child_names = Vec::new();
    loop {
        unsafe {
            *libc::__errno_location() = 0;
        }
        let entry = unsafe { libc::readdir(dirp) };
        if entry.is_null() {
            let err = unsafe { *libc::__errno_location() };
            unsafe {
                libc::closedir(dirp);
            }
            if err != 0 {
                return Err(HumanTaskStoreError::Unavailable);
            }
            break;
        }
        let d_name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let child = match d_name.to_str() {
            Ok("." | "..") => continue,
            Ok(value) => value.to_string(),
            Err(_) => {
                unsafe {
                    libc::closedir(dirp);
                }
                return Err(HumanTaskStoreError::Invalid);
            }
        };
        child_names.push(child);
    }

    for child in &child_names {
        let child_c = CString::new(child.as_str()).map_err(|_| HumanTaskStoreError::Invalid)?;
        let mut child_st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::fstatat(
                dir_fd,
                child_c.as_ptr(),
                &mut child_st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(HumanTaskStoreError::Unavailable);
        }
        let child_type = child_st.st_mode & libc::S_IFMT;
        let removable =
            child_type == libc::S_IFLNK || (child_type == libc::S_IFREG && child_st.st_uid == euid);
        if !removable {
            return Err(HumanTaskStoreError::PermissionDenied);
        }
    }
    Ok(child_names)
}

fn after_root_pin_hook(_root: &Path) {
    #[cfg(test)]
    if let Ok(guard) = test_hooks::AFTER_ROOT_PIN.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(_root);
        }
    }
}

fn after_quarantine_hook(_root: &Path, _quarantine_name: &str) {
    #[cfg(test)]
    if let Ok(guard) = test_hooks::AFTER_QUARANTINE.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(_root, _quarantine_name);
        }
    }
}

fn after_directory_pin_hook(_root: &Path, _quarantine_name: &str) {
    #[cfg(test)]
    if let Ok(guard) = test_hooks::AFTER_DIRECTORY_PIN.lock() {
        if let Some(hook) = guard.as_ref() {
            hook(_root, _quarantine_name);
        }
    }
}

#[cfg(test)]
pub mod test_hooks {
    use std::path::Path;
    use std::sync::Mutex;

    type RootHook = Box<dyn Fn(&Path) + Send>;
    type Hook = Box<dyn Fn(&Path, &str) + Send>;

    pub static AFTER_ROOT_PIN: Mutex<Option<RootHook>> = Mutex::new(None);
    pub static AFTER_QUARANTINE: Mutex<Option<Hook>> = Mutex::new(None);
    pub static AFTER_DIRECTORY_PIN: Mutex<Option<Hook>> = Mutex::new(None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn write_invalid_residue(history: &Path) -> PathBuf {
        let root = history.join("human-tasks");
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let task_dir = root.join("ht-20260718-aabbcc");
        fs::create_dir(&task_dir).unwrap();
        fs::set_permissions(&task_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let checkpoint = task_dir.join("checkpoint.json");
        fs::write(&checkpoint, b"not-json").unwrap();
        fs::set_permissions(&checkpoint, fs::Permissions::from_mode(0o600)).unwrap();
        task_dir
    }

    #[test]
    fn remove_invalid_active_preserves_replacement_after_directory_pin() {
        let _guard = TEST_LOCK.lock().unwrap();
        let history = tempfile::tempdir().unwrap();
        let task_dir = write_invalid_residue(history.path());
        let outside = tempfile::tempdir().unwrap();
        let store = HumanTaskFileStore::new(history.path().into());

        *test_hooks::AFTER_DIRECTORY_PIN.lock().unwrap() = Some(Box::new({
            let outside = outside.path().to_path_buf();
            move |root, quarantine_name| {
                let quarantine = root.join(".removing").join(quarantine_name);
                let moved = outside.join("moved-residue");
                fs::rename(&quarantine, &moved).unwrap();
                fs::create_dir(&quarantine).unwrap();
                fs::set_permissions(&quarantine, fs::Permissions::from_mode(0o700)).unwrap();
                let keep = quarantine.join("keep-me.txt");
                fs::write(&keep, b"do-not-delete").unwrap();
                fs::set_permissions(&keep, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }));

        let result = store.remove_invalid_active();
        *test_hooks::AFTER_DIRECTORY_PIN.lock().unwrap() = None;
        result.expect("cleanup should succeed without removing replacement");

        assert!(!task_dir.exists());
        let decoy = history
            .path()
            .join("human-tasks")
            .join(".removing")
            .read_dir()
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.join("keep-me.txt").exists())
            .expect("replacement decoy must remain");
        assert_eq!(
            fs::read(decoy.join("keep-me.txt")).unwrap(),
            b"do-not-delete"
        );
        let moved = outside.path().join("moved-residue");
        assert!(moved.exists());
        assert!(!moved.join("checkpoint.json").exists());
    }

    #[test]
    fn remove_invalid_active_keeps_outer_tree_when_root_path_replaced() {
        let _guard = TEST_LOCK.lock().unwrap();
        let history = tempfile::tempdir().unwrap();
        let _task_dir = write_invalid_residue(history.path());
        let outside = tempfile::tempdir().unwrap();
        let fake_root = outside.path().join("fake-human-tasks");
        fs::create_dir_all(&fake_root).unwrap();
        fs::set_permissions(&fake_root, fs::Permissions::from_mode(0o700)).unwrap();
        let fake_task = fake_root.join("ht-20260718-aabbcc");
        fs::create_dir(&fake_task).unwrap();
        fs::set_permissions(&fake_task, fs::Permissions::from_mode(0o700)).unwrap();
        let keep = fake_task.join("keep-outside.txt");
        fs::write(&keep, b"outer-secret").unwrap();
        fs::set_permissions(&keep, fs::Permissions::from_mode(0o600)).unwrap();

        let store = HumanTaskFileStore::new(history.path().into());
        *test_hooks::AFTER_ROOT_PIN.lock().unwrap() = Some(Box::new({
            let history = history.path().to_path_buf();
            let fake_root = fake_root.clone();
            let parked = outside.path().join("parked-root");
            move |root| {
                assert_eq!(root, &history.join("human-tasks"));
                fs::rename(root, &parked).unwrap();
                symlink(&fake_root, root).unwrap();
            }
        }));

        let result = store.remove_invalid_active();
        *test_hooks::AFTER_ROOT_PIN.lock().unwrap() = None;
        result.expect("pinned root cleanup should succeed");

        assert_eq!(fs::read(&keep).unwrap(), b"outer-secret");
        assert!(keep.exists());
    }

    #[test]
    fn remove_invalid_active_rejects_nested_directory_without_mutating_residue() {
        let _guard = TEST_LOCK.lock().unwrap();
        let history = tempfile::tempdir().unwrap();
        let task_dir = write_invalid_residue(history.path());
        let nested = task_dir.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        let checkpoint = task_dir.join("checkpoint.json");
        let before = fs::read(&checkpoint).unwrap();

        let store = HumanTaskFileStore::new(history.path().into());
        assert_eq!(
            store.remove_invalid_active(),
            Err(HumanTaskStoreError::PermissionDenied)
        );
        assert!(task_dir.exists());
        assert_eq!(fs::read(&checkpoint).unwrap(), before);
        assert!(nested.exists());
        assert!(!history
            .path()
            .join("human-tasks")
            .join(".removing")
            .exists());
    }
}
