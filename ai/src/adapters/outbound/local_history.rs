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

    fn prune_to_max(&self, max_entries: usize) -> Result<usize, HistoryStoreError> {
        if max_entries == 0 {
            return Ok(0);
        }
        let entries = self.list()?;
        if entries.len() <= max_entries {
            return Ok(0);
        }
        let drop_count = entries.len() - max_entries;
        let to_drop = entries[max_entries..].to_vec();
        let kept = entries[..max_entries].to_vec();
        self.rewrite_index(&kept)?;
        for entry in &to_drop {
            let path = self.payload_path(&entry.history_id);
            let _ = fs::remove_file(path);
        }
        Ok(drop_count)
    }
}

impl LocalHistoryStore {
    fn rewrite_index(&self, entries: &[HistoryIndexEntry]) -> Result<(), HistoryStoreError> {
        self.ensure_layout()?;
        let path = self.index_path();
        let temp = self.root.join("index.jsonl.tmp");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)
            .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        let mut ordered = entries.to_vec();
        ordered.sort_by(|a, b| {
            a.created_at_ms
                .cmp(&b.created_at_ms)
                .then_with(|| a.history_id.cmp(&b.history_id))
        });
        for entry in ordered {
            let line = serde_json::to_string(&entry)
                .map_err(|e| HistoryStoreError::Write(e.to_string()))?;
            writeln!(file, "{line}").map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        }
        drop(file);
        set_permissions_0600(&temp).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        fs::rename(&temp, &path).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        set_permissions_0600(&path).map_err(|e| HistoryStoreError::Write(e.to_string()))?;
        Ok(())
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

    #[test]
    fn prune_to_max_drops_oldest_payloads() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LocalHistoryStore::new(dir.path().to_path_buf());
        for (idx, id) in ["a", "b", "c", "d"].into_iter().enumerate() {
            let entry = HistoryIndexEntry {
                history_id: id.into(),
                created_at_ms: idx as u64,
                command: "ask".into(),
                session_id: None,
                conversation_id: None,
                preset: None,
                profile: None,
                shell_exec_approval: None,
                socket_path: "/tmp/s".into(),
                request_kind: crate::domain::HistoryRecordKind::Ask,
                request_summary: crate::domain::HistorySummary::new("req"),
                response_kind: crate::domain::HistoryRecordKind::Ask,
                response_summary: crate::domain::HistorySummary::new("resp"),
                status: crate::domain::HistoryRecordStatus::Ok,
            };
            let payload = HistoryPayload {
                history_id: id.into(),
                command: "ask".into(),
                user_message: id.into(),
                request_messages: vec![],
                shell_log_tail: None,
                client_cwd: None,
                tools: vec![],
                llm_profile: None,
                preset: None,
                session_id: None,
                conversation_id: None,
                shell_exec_approval: None,
                socket_path: "/tmp/s".into(),
                log_tail_bytes: 1,
            };
            store.append(&entry, &payload).expect("append");
        }
        let removed = store.prune_to_max(3).expect("prune");
        assert_eq!(removed, 1);
        let entries = store.list().expect("list");
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().any(|e| e.history_id == "d"));
        assert!(store.load_payload("a").is_err());
        assert!(store.load_payload("d").is_ok());
    }
}
