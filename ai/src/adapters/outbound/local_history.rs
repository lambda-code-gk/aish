//! local history のファイルストア。

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::domain::{HistoryIndexEntry, HistoryPayload};
use crate::ports::outbound::{HistoryStore, HistoryStoreError};

#[derive(Debug, Clone)]
pub struct LocalHistoryStore {
    root: PathBuf,
}

impl LocalHistoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.jsonl")
    }

    fn payload_dir(&self) -> PathBuf {
        self.root.join("payloads")
    }

    fn payload_path(&self, history_id: &str) -> PathBuf {
        self.payload_dir().join(format!("{history_id}.json"))
    }

    fn ensure_layout(&self) -> Result<(), HistoryStoreError> {
        create_dir_0700(&self.root).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        create_dir_0700(&self.payload_dir())
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        Ok(())
    }
}

impl HistoryStore for LocalHistoryStore {
    fn append(
        &self,
        entry: &HistoryIndexEntry,
        payload: &HistoryPayload,
    ) -> Result<(), HistoryStoreError> {
        self.ensure_layout()?;

        let payload_path = self.payload_path(&entry.history_id);
        let mut payload_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&payload_path)
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        let payload_json =
            serde_json::to_string(payload).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        writeln!(payload_file, "{payload_json}")
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        set_permissions_0600(&payload_path).map_err(|e| HistoryStoreError::Write(e.to_string()))?;

        let mut index = open_append_0600(&self.index_path())
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        let line =
            serde_json::to_string(entry).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        writeln!(index, "{line}").map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        set_permissions_0600(&self.index_path())
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<HistoryIndexEntry>, HistoryStoreError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&path).map_err(|e| HistoryStoreError::Read(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| HistoryStoreError::Read(e.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: HistoryIndexEntry =
                serde_json::from_str(&line).map_err(|e| HistoryStoreError::Read(e.to_string()))?;
            entries.push(entry);
        }
        entries.sort_by(|a, b| {
            b.created_at_ms
                .cmp(&a.created_at_ms)
                .then_with(|| b.history_id.cmp(&a.history_id))
        });
        Ok(entries)
    }

    fn load_payload(&self, history_id: &str) -> Result<HistoryPayload, HistoryStoreError> {
        let path = self.payload_path(history_id);
        if !path.exists() {
            return Err(HistoryStoreError::NotFound(path.display().to_string()));
        }
        let raw = fs::read_to_string(&path).map_err(|e| HistoryStoreError::Read(e.to_string()))?;
        let payload: HistoryPayload =
            serde_json::from_str(raw.trim()).map_err(|e| HistoryStoreError::Read(e.to_string()))?;
        Ok(payload)
    }
}

fn open_append_0600(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    set_permissions_0600(path)?;
    Ok(file)
}

fn create_dir_0700(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    set_permissions_0700(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_permissions_0600(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_permissions_0600(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_permissions_0700(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn set_permissions_0700(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_load_payload_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LocalHistoryStore::new(dir.path().to_path_buf());
        let entry = HistoryIndexEntry {
            history_id: "abc".into(),
            created_at_ms: 1,
            command: "ask".into(),
            session_id: Some("sess".into()),
            conversation_id: None,
            preset: Some("fast".into()),
            profile: Some("fast".into()),
            shell_exec_approval: Some("ask".into()),
            socket_path: "/tmp/sock".into(),
            request_kind: crate::domain::HistoryRecordKind::Ask,
            request_summary: crate::domain::HistorySummary::new("req"),
            response_kind: crate::domain::HistoryRecordKind::Ask,
            response_summary: crate::domain::HistorySummary::new("resp"),
            status: crate::domain::HistoryRecordStatus::Ok,
        };
        let payload = HistoryPayload {
            history_id: "abc".into(),
            command: "ask".into(),
            user_message: "hello".into(),
            shell_log_tail: Some("tail".into()),
            client_cwd: Some("/tmp".into()),
            tools: vec!["read_file".into()],
            llm_profile: Some("fast".into()),
            preset: Some("fast".into()),
            session_id: Some("sess".into()),
            conversation_id: Some("conv".into()),
            shell_exec_approval: Some("ask".into()),
            socket_path: "/tmp/sock".into(),
            log_tail_bytes: 16,
            request_messages: vec![],
        };
        store.append(&entry, &payload).expect("append");
        let entries = store.list().expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].history_id, "abc");
        let loaded = store.load_payload("abc").expect("payload");
        assert_eq!(loaded.user_message, "hello");
    }
}
