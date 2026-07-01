//! suggested command recall cache のファイルストア。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::SuggestedCommandCache;
use crate::ports::outbound::{SuggestedCommandRecallStore, SuggestedCommandRecallStoreError};

#[derive(Debug, Clone)]
pub struct FileSuggestedCommandRecallStore {
    path: PathBuf,
}

impl FileSuggestedCommandRecallStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

pub fn default_suggestion_cache_path(ai_session_id: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local/share/ai/suggestions")
        .join(format!("{ai_session_id}.json"))
}

pub fn resolve_suggestion_cache_path(ai_session_id: &str) -> PathBuf {
    std::env::var("AI_SUGGESTION_CACHE")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| default_suggestion_cache_path(ai_session_id))
}

impl SuggestedCommandRecallStore for FileSuggestedCommandRecallStore {
    fn load(&self) -> Result<Option<SuggestedCommandCache>, SuggestedCommandRecallStoreError> {
        if !self.path.is_file() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.path)
            .map_err(|e| SuggestedCommandRecallStoreError::Read(e.to_string()))?;
        let cache: SuggestedCommandCache = serde_json::from_str(raw.trim())
            .map_err(|e| SuggestedCommandRecallStoreError::Read(e.to_string()))?;
        if cache.schema_version != 1 {
            return Ok(None);
        }
        Ok(Some(cache))
    }

    fn save(&self, cache: &SuggestedCommandCache) -> Result<(), SuggestedCommandRecallStoreError> {
        if let Some(parent) = self.path.parent() {
            create_dir_0700(parent)
                .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        }
        let json = serde_json::to_string_pretty(cache)
            .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        let temp = self.path.with_extension("json.tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp)
                .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
            file.write_all(json.as_bytes())
                .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
            file.sync_all()
                .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        }
        set_permissions_0600(&temp)
            .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        fs::rename(&temp, &self.path)
            .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        set_permissions_0600(&self.path)
            .map_err(|e| SuggestedCommandRecallStoreError::Write(e.to_string()))?;
        Ok(())
    }

    fn cache_path(&self) -> &Path {
        &self.path
    }
}

fn create_dir_0700(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    set_permissions_0700(path)
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
    use crate::domain::{SuggestedCommandCandidate, SuggestedCommandQueue};

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("cache.json");
        let store = FileSuggestedCommandRecallStore::new(path.clone());
        let mut cache = SuggestedCommandCache::new("sess", "bash", "2026-07-01T00:00:00Z");
        cache.append_queue(SuggestedCommandQueue {
            turn_id: "t1".into(),
            captured_at: "2026-07-01T00:00:00Z".into(),
            candidates: vec![SuggestedCommandCandidate {
                text: "git status".into(),
                language: "bash".into(),
                bytes: 10,
            }],
        });
        store.save(&cache).expect("save");
        let loaded = store.load().expect("load").expect("some");
        assert_eq!(loaded.queues.len(), 1);
        assert_eq!(loaded.queues[0].candidates[0].text, "git status");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }
}
