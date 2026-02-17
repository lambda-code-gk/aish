//! 永続メモリの一覧・取得・削除の標準実装
//!
//! ai の metadata.json / entries/<id>.json と同一形式を読む。

use crate::domain::{MemoryEntry, MemoryListEntry};
use crate::ports::outbound::MemoryRepository;
use common::error::Error;
use common::ports::outbound::EnvResolver;
use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MEMORY_SUBDIR: &str = "memory";
const AISH_DIR: &str = ".aish";
const METADATA_FILENAME: &str = "metadata.json";
const ENTRIES_DIR: &str = "entries";

pub struct StdMemoryRepository {
    env: Arc<dyn EnvResolver>,
}

impl StdMemoryRepository {
    pub fn new(env: Arc<dyn EnvResolver>) -> Self {
        Self { env }
    }
}

impl MemoryRepository for StdMemoryRepository {
    fn resolve(&self) -> Result<(Option<PathBuf>, PathBuf), Error> {
        // ディレクトリ解決は EnvResolver::resolve_dirs() に集約し、home を data/config の「root」として扱わない
        let dirs = self.env.resolve_dirs()?;
        let global = dirs.data_dir.join(MEMORY_SUBDIR);
        let project = find_project_memory_dir(self.env.current_dir()?.as_path())?;
        Ok((project, global))
    }

    fn list(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
    ) -> Result<Vec<MemoryListEntry>, Error> {
        let mut out = Vec::new();
        for (dir, _source) in [(project_dir, "project"), (Some(global_dir), "global")] {
            let Some(d) = dir else { continue };
            if project_dir == Some(global_dir) && d == global_dir {
                continue;
            }
            let metas = load_metadata(d)?;
            for m in metas {
                out.push(MemoryListEntry {
                    id: m.id,
                    category: m.category,
                    keywords: m.keywords,
                    subject: m.subject,
                    timestamp: m.timestamp,
                });
            }
        }
        Ok(out)
    }

    fn get(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<MemoryEntry, Error> {
        if let Some(d) = project_dir {
            if let Ok(e) = read_entry_file(d, id) {
                return Ok(e);
            }
        }
        read_entry_file(global_dir, id)
    }

    fn remove(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<(), Error> {
        let dir = if let Some(d) = project_dir {
            if entry_file_exists(d, id) {
                d
            } else if entry_file_exists(global_dir, id) {
                global_dir
            } else {
                return Err(Error::io_msg(format!("memory not found: {}", id)));
            }
        } else if entry_file_exists(global_dir, id) {
            global_dir
        } else {
            return Err(Error::io_msg(format!("memory not found: {}", id)));
        };
        remove_entry_file(dir, id)?;
        remove_from_metadata(dir, id)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct MetadataFile {
    memories: Vec<MemoryMeta>,
}

#[derive(Debug, Deserialize)]
struct MemoryMeta {
    id: String,
    category: String,
    #[serde(default)]
    keywords: Vec<String>,
    subject: String,
    timestamp: String,
}

fn find_project_memory_dir(mut current: &Path) -> Result<Option<PathBuf>, Error> {
    loop {
        let candidate = current.join(AISH_DIR).join(MEMORY_SUBDIR);
        if candidate.exists() {
            let meta = std::fs::metadata(&candidate)
                .map_err(|e| Error::io_msg(format!("metadata {}: {}", candidate.display(), e)))?;
            if meta.is_dir() {
                return Ok(Some(candidate));
            }
        }
        match current.parent() {
            Some(p) => current = p,
            None => return Ok(None),
        }
    }
}

fn load_metadata(dir: &Path) -> Result<Vec<MemoryMeta>, Error> {
    let path = dir.join(METADATA_FILENAME);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let s = std::fs::read_to_string(&path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let meta: MetadataFile =
        serde_json::from_str(&s).map_err(|e| Error::io_msg(format!("parse {}: {}", path.display(), e)))?;
    Ok(meta.memories)
}

fn entry_file_exists(dir: &Path, id: &str) -> bool {
    dir.join(ENTRIES_DIR).join(format!("{}.json", id)).exists()
}

fn read_entry_file(dir: &Path, id: &str) -> Result<MemoryEntry, Error> {
    let path = dir.join(ENTRIES_DIR).join(format!("{}.json", id));
    if !path.exists() {
        return Err(Error::io_msg(format!("memory not found: {}", id)));
    }
    let s = std::fs::read_to_string(&path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let e: MemoryEntry = serde_json::from_str(&s)
        .map_err(|e| Error::io_msg(format!("parse {}: {}", path.display(), e)))?;
    Ok(e)
}

fn remove_entry_file(dir: &Path, id: &str) -> Result<(), Error> {
    let path = dir.join(ENTRIES_DIR).join(format!("{}.json", id));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| Error::io_msg(format!("remove {}: {}", path.display(), e)))?;
    }
    Ok(())
}

/// metadata.json を読み、指定 id を除いて書き戻す（既存フィールドを保持するため Value で扱う）
fn remove_from_metadata(dir: &Path, id: &str) -> Result<(), Error> {
    let path = dir.join(METADATA_FILENAME);
    if !path.exists() {
        return Ok(());
    }
    let s = std::fs::read_to_string(&path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let mut meta: Value = serde_json::from_str(&s)
        .map_err(|e| Error::io_msg(format!("parse {}: {}", path.display(), e)))?;
    let memories = meta
        .get_mut("memories")
        .and_then(|m| m.as_array_mut())
        .ok_or_else(|| Error::io_msg("metadata: missing memories array".to_string()))?;
    memories.retain(|v| v.get("id").and_then(|x| x.as_str()) != Some(id));
    let updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    meta["last_updated"] = serde_json::json!(updated);
    std::fs::write(&path, serde_json::to_string_pretty(&meta).unwrap())
        .map_err(|e| Error::io_msg(format!("write {}: {}", path.display(), e)))?;
    Ok(())
}
