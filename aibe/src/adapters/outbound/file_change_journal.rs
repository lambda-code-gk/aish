//! rollback journal filesystem adapter（設計 §19）。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::BeforeState;
use crate::ports::outbound::file_change_journal::{
    FileChangeJournal, FileChangeJournalError, JournalEntry, JournalSaveRequest,
};

use super::secure_fs::{create_dir_0700, set_permissions_0600};

static CHANGE_SEQ: AtomicU64 = AtomicU64::new(0);

/// journal 設定。
#[derive(Debug, Clone)]
pub struct FileChangeJournalConfig {
    pub root: PathBuf,
    pub retention_days: u32,
    pub max_bytes: u64,
}

impl FileChangeJournalConfig {
    pub fn default_root() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".local/share/aibe")
            .join("file-changes")
    }
}

/// filesystem rollback journal。
#[derive(Debug, Clone)]
pub struct FilesystemFileChangeJournal {
    config: FileChangeJournalConfig,
}

impl FilesystemFileChangeJournal {
    pub fn new(config: FileChangeJournalConfig) -> Self {
        Self { config }
    }

    pub(crate) fn save_before_sync(
        &self,
        request: JournalSaveRequest,
    ) -> Result<JournalEntry, FileChangeJournalError> {
        let before_bin_len = request.before_bytes.as_ref().map_or(0, |b| b.len() as u64);
        let metadata_len = estimate_metadata_len(&request);
        self.ensure_capacity(before_bin_len.saturating_add(metadata_len))?;

        let change_id = next_change_id();
        let date_dir = utc_date_dir();
        let entry_dir = self.config.root.join(&date_dir).join(&change_id);
        create_dir_0700(&self.config.root).map_err(|_| FileChangeJournalError::Failed)?;
        create_dir_0700(&self.config.root.join(&date_dir))
            .map_err(|_| FileChangeJournalError::Failed)?;
        create_dir_0700(&entry_dir).map_err(|_| FileChangeJournalError::Failed)?;

        if request.before_state == BeforeState::Present {
            let bytes = request
                .before_bytes
                .as_ref()
                .ok_or(FileChangeJournalError::Failed)?;
            let before_path = entry_dir.join("before.bin");
            write_private_file(&before_path, bytes).map_err(|_| FileChangeJournalError::Failed)?;
        }

        let metadata = JournalMetadata {
            change_id: change_id.clone(),
            created_at: rfc3339_now(),
            tool: request.tool,
            target_path: request.target_path.display().to_string(),
            before_state: match request.before_state {
                BeforeState::Absent => "absent".to_string(),
                BeforeState::Present => "present".to_string(),
            },
            before_sha256: request.before_sha256,
            after_sha256: request.after_sha256,
            before_bytes: request.before_bytes.as_ref().map_or(0, |b| b.len()),
            after_bytes: request.after_bytes,
            file_mode: request.file_mode,
            operation: request.operation.as_str().to_string(),
            status: "prepared".to_string(),
        };

        let metadata_json =
            serde_json::to_string_pretty(&metadata).map_err(|_| FileChangeJournalError::Failed)?;
        write_private_file(
            entry_dir.join("metadata.json").as_path(),
            metadata_json.as_bytes(),
        )
        .map_err(|_| FileChangeJournalError::Failed)?;

        Ok(JournalEntry {
            change_id,
            dir: entry_dir,
        })
    }

    pub(crate) fn cleanup_expired_sync(&self) -> Result<(), FileChangeJournalError> {
        if !self.config.root.exists() {
            return Ok(());
        }
        let cutoff = journal_cutoff_secs(self.config.retention_days);
        let dates = fs::read_dir(&self.config.root).map_err(|_| FileChangeJournalError::Failed)?;
        for date_entry in dates.flatten() {
            let date_path = date_entry.path();
            if !date_path.is_dir() {
                continue;
            }
            let changes = fs::read_dir(&date_path).map_err(|_| FileChangeJournalError::Failed)?;
            for change_entry in changes.flatten() {
                let change_path = change_entry.path();
                if !change_path.is_dir() {
                    continue;
                }
                let meta_path = change_path.join("metadata.json");
                let remove = if meta_path.is_file() {
                    match read_metadata_created_at(&meta_path) {
                        Some(created) => created < cutoff,
                        None => true,
                    }
                } else {
                    true
                };
                if remove {
                    let _ = fs::remove_dir_all(&change_path);
                }
            }
            if fs::read_dir(&date_path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = fs::remove_dir(&date_path);
            }
        }
        Ok(())
    }

    pub(crate) fn mark_status_sync(
        &self,
        entry: &JournalEntry,
        status: &str,
    ) -> Result<(), FileChangeJournalError> {
        let meta_path = entry.dir.join("metadata.json");
        let text = fs::read_to_string(&meta_path).map_err(|_| FileChangeJournalError::Failed)?;
        let mut value: serde_json::Value =
            serde_json::from_str(&text).map_err(|_| FileChangeJournalError::Failed)?;
        let Some(obj) = value.as_object_mut() else {
            return Err(FileChangeJournalError::Failed);
        };
        obj.insert(
            "status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        let updated =
            serde_json::to_string_pretty(&value).map_err(|_| FileChangeJournalError::Failed)?;
        atomic_write_private_file(&meta_path, updated.as_bytes())
            .map_err(|_| FileChangeJournalError::Failed)
    }

    fn ensure_capacity(&self, incoming_bytes: u64) -> Result<(), FileChangeJournalError> {
        self.cleanup_expired_sync()?;
        let used = dir_size(&self.config.root).unwrap_or(0);
        if used.saturating_add(incoming_bytes) > self.config.max_bytes {
            return Err(FileChangeJournalError::CapacityExceeded);
        }
        Ok(())
    }
}

#[async_trait]
impl FileChangeJournal for FilesystemFileChangeJournal {
    async fn save_before(
        &self,
        request: JournalSaveRequest,
    ) -> Result<JournalEntry, FileChangeJournalError> {
        self.save_before_sync(request)
    }

    async fn cleanup_expired(&self) -> Result<(), FileChangeJournalError> {
        self.cleanup_expired_sync()
    }

    async fn mark_status(
        &self,
        entry: &JournalEntry,
        status: &str,
    ) -> Result<(), FileChangeJournalError> {
        self.mark_status_sync(entry, status)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalMetadata {
    change_id: String,
    created_at: String,
    tool: String,
    target_path: String,
    before_state: String,
    before_sha256: Option<String>,
    after_sha256: String,
    before_bytes: usize,
    after_bytes: usize,
    file_mode: Option<u32>,
    operation: String,
    status: String,
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    set_permissions_0600(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn atomic_write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    super::secure_fs::atomic_write_in_place(path, bytes, Some(0o600), ".metadata-")
}

fn next_change_id() -> String {
    let seq = CHANGE_SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("chg_{ms:016x}_{seq:04x}")
}

fn utc_date_dir() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let day_secs = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day_secs / 3600,
        (day_secs % 3600) / 60,
        day_secs % 60
    )
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn civil_to_days(y: i32, m: u32, d: u32) -> Option<i64> {
    let y = i64::from(y);
    let m = i64::from(m);
    let d = i64::from(d);
    let y = y - (if m <= 2 { 1 } else { 0 });
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) as u32 + 2) / 5 + d as u32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe as i64 - 719_468)
}

fn journal_cutoff_secs(retention_days: u32) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(u64::from(retention_days) * 86_400)
}

fn read_metadata_created_at(path: &Path) -> Option<u64> {
    let text = fs::read_to_string(path).ok()?;
    let meta: JournalMetadata = serde_json::from_str(&text).ok()?;
    parse_rfc3339_secs(&meta.created_at)
}

fn parse_rfc3339_secs(value: &str) -> Option<u64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let (y, rest) = date.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let (hh, rest) = time.split_once(':')?;
    let (mm, ss) = rest.split_once(':')?;
    let days = civil_to_days(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)?;
    Some(
        (days as u64) * 86_400
            + hh.parse::<u64>().ok()? * 3600
            + mm.parse::<u64>().ok()? * 60
            + ss.parse::<u64>().ok()?,
    )
}

fn dir_size(path: &Path) -> std::io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path())?);
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    Ok(total)
}

fn estimate_metadata_len(request: &JournalSaveRequest) -> u64 {
    512 + request.target_path.display().to_string().len() as u64
}

/// metadata.json を読む（テスト用）。
pub fn read_journal_metadata(entry_dir: &Path) -> std::io::Result<serde_json::Value> {
    let text = fs::read_to_string(entry_dir.join("metadata.json"))?;
    Ok(serde_json::from_str(&text)?)
}

/// テスト用: created_at を過去に書き換える。
pub fn set_journal_created_at_for_test(entry_dir: &Path, created_at: &str) -> std::io::Result<()> {
    let meta_path = entry_dir.join("metadata.json");
    let text = fs::read_to_string(&meta_path)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)?;
    value["created_at"] = serde_json::Value::String(created_at.to_string());
    write_private_file(&meta_path, serde_json::to_string_pretty(&value)?.as_bytes())
}

/// path の permission モード（テスト用）。
pub fn path_mode(path: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{sha256_hex, FileChangeOperation};

    #[test]
    fn save_absent_before_has_no_bin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = FilesystemFileChangeJournal::new(FileChangeJournalConfig {
            root: dir.path().to_path_buf(),
            retention_days: 7,
            max_bytes: 1_000_000,
        });
        let entry = journal
            .save_before_sync(JournalSaveRequest {
                tool: "write_file".to_string(),
                target_path: PathBuf::from("/tmp/new.txt"),
                before_state: BeforeState::Absent,
                before_bytes: None,
                before_sha256: None,
                after_sha256: sha256_hex(b"new"),
                after_bytes: 3,
                file_mode: None,
                operation: FileChangeOperation::Create,
            })
            .expect("save");
        assert!(!entry.dir.join("before.bin").exists());
    }
}
