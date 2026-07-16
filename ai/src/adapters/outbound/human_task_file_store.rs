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
}

impl HumanTaskStore for HumanTaskFileStore {
    fn lock_exclusive(&self) -> Result<Box<dyn HumanTaskStoreLock + '_>, HumanTaskStoreError> {
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
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(HumanTaskStoreError::Unavailable);
        }
        Ok(Box::new(HumanTaskFileLock(file)))
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
            if name == "lock" {
                continue;
            }
            let id = HumanTaskId::parse(name).map_err(|_| HumanTaskStoreError::Invalid)?;
            let dir = entry.path();
            Self::ensure_dir(&dir)?;
            let checkpoint = Self::read_checkpoint(&dir.join("checkpoint.json"))?;
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
}
