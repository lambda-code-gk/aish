//! 永続メモリのファイル読み書き（metadata.json + entries/<id>.json）
//!
//! 旧実装と互換の形式。ツールから利用する stateless なヘルパー。
//! オプションで Log を渡すとメモリの読み書きをログに記録する。

use crate::domain::{MemoryEntry, MemoryMeta};
use common::error::Error;
use common::ports::outbound::{Log, LogLevel, LogRecord, now_iso8601};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const METADATA_FILENAME: &str = "metadata.json";
const ENTRIES_DIR: &str = "entries";

/// 保存先ディレクトリを初期化（entries 作成、metadata.json が無ければ作成）
pub fn init_memory_dir(dir: &Path) -> Result<(), Error> {
    let entries = dir.join(ENTRIES_DIR);
    std::fs::create_dir_all(&entries)
        .map_err(|e| Error::io_msg(format!("create_dir_all {}: {}", entries.display(), e)))?;
    let meta_path = dir.join(METADATA_FILENAME);
    if !meta_path.exists() {
        let initial = serde_json::json!({
            "memories": [],
            "last_updated": null,
            "memory_dir": dir.to_string_lossy()
        });
        std::fs::write(&meta_path, serde_json::to_string_pretty(&initial).unwrap())
            .map_err(|e| Error::io_msg(format!("write {}: {}", meta_path.display(), e)))?;
    }
    Ok(())
}

/// 16文字 hex のメモリ ID を生成（旧実装互換）
fn generate_memory_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    format!("{:016x}", nanos)
}

/// 1 件保存。dir を初期化し、entries/<id>.json と metadata に追加。
/// log に Some を渡すとメモリ書き込みをログに記録する。
pub fn save_entry(
    dir: &Path,
    entry: &MemoryEntry,
    log: Option<&dyn Log>,
) -> Result<String, Error> {
    init_memory_dir(dir)?;
    let id = if entry.id.is_empty() {
        generate_memory_id()
    } else {
        entry.id.clone()
    };
    let mut e = entry.clone();
    e.id = id.clone();
    let entry_path = dir.join(ENTRIES_DIR).join(format!("{}.json", id));
    let json = serde_json::to_string_pretty(&e).map_err(|e| Error::io_msg(e.to_string()))?;
    std::fs::write(&entry_path, json)
        .map_err(|e| Error::io_msg(format!("write {}: {}", entry_path.display(), e)))?;

    let meta_path = dir.join(METADATA_FILENAME);
    let meta: MetadataFile = read_metadata(&meta_path)?;
    let mut memories = meta.memories;
    memories.push(MemoryMeta {
        id: e.id.clone(),
        category: e.category.clone(),
        keywords: e.keywords.clone(),
        subject: e.subject.clone(),
        timestamp: e.timestamp.clone(),
        usage_count: e.usage_count,
        memory_dir: Some(dir.to_string_lossy().to_string()),
        project_root: e.project_root.clone(),
        source: None,
        score: None,
    });
    write_metadata(&meta_path, &memories, dir)?;
    if let Some(logger) = log {
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("write"));
        fields.insert("memory_id".to_string(), serde_json::json!(id));
        fields.insert("dir".to_string(), serde_json::json!(dir.to_string_lossy().to_string()));
        fields.insert("category".to_string(), serde_json::json!(e.category));
        let _ = logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory write".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
    }
    Ok(id)
}

#[derive(Debug, Deserialize)]
struct MetadataFile {
    memories: Vec<MemoryMeta>,
}

fn read_metadata(path: &Path) -> Result<MetadataFile, Error> {
    if !path.exists() {
        return Ok(MetadataFile {
            memories: Vec::new(),
        });
    }
    let s = std::fs::read_to_string(path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let meta: MetadataFile = serde_json::from_str(&s).map_err(|e| Error::io_msg(e.to_string()))?;
    Ok(meta)
}

fn write_metadata(path: &Path, memories: &[MemoryMeta], memory_dir: &Path) -> Result<(), Error> {
    let j = serde_json::json!({
        "memories": memories,
        "last_updated": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64(),
        "memory_dir": memory_dir.to_string_lossy()
    });
    std::fs::write(path, serde_json::to_string_pretty(&j).unwrap())
        .map_err(|e| Error::io_msg(format!("write {}: {}", path.display(), e)))?;
    Ok(())
}

/// 1 ディレクトリ内の全メタデータを読み込み。
/// log に Some を渡すとメモリ読み込みをログに記録する。
pub fn load_metadata(dir: &Path, log: Option<&dyn Log>) -> Result<Vec<MemoryMeta>, Error> {
    let path = dir.join(METADATA_FILENAME);
    let meta = read_metadata(&path)?;
    if let Some(logger) = log {
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("load_metadata"));
        fields.insert("dir".to_string(), serde_json::json!(dir.to_string_lossy().to_string()));
        fields.insert("count".to_string(), serde_json::json!(meta.memories.len()));
        let _ = logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory read".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
    }
    Ok(meta.memories)
}

/// 複数ディレクトリ（project, global）を検索し、キーワード・subject でマッチするものをスコア付きで返す。
/// include_content が true のときは entries/<id>.json から content を補完する。
/// log に Some を渡すとメモリ読み込みをログに記録する。
pub fn search_entries(
    project_dir: Option<&Path>,
    global_dir: &Path,
    query: &str,
    category_filter: Option<&str>,
    limit: usize,
    include_content: bool,
    log: Option<&dyn Log>,
) -> Result<Vec<MemoryEntry>, Error> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(MemoryMeta, std::path::PathBuf, String, u64)> =
        Vec::new(); // (meta, dir, source, score)

    for (dir, source) in [(project_dir, "project"), (Some(global_dir), "global")] {
        let Some(d) = dir else { continue };
        if project_dir == Some(global_dir) && source == "global" {
            continue;
        }
        let metas = load_metadata(d, None)?;
        for m in metas {
            if let Some(cat) = category_filter {
                if !cat.is_empty() && m.category != cat {
                    continue;
                }
            }
            let mut score = 0u64;
            for kw in &m.keywords {
                let kw_lower = kw.to_lowercase();
                if query_lower.contains(&kw_lower) {
                    score += 1;
                }
            }
            if m.subject.to_lowercase().contains(&query_lower) {
                score += 2;
            }
            if score > 0 {
                scored.push((m, d.to_path_buf(), source.to_string(), score));
            }
        }
    }

    scored.sort_by(|a, b| b.3.cmp(&a.3));
    let top: Vec<_> = scored.into_iter().take(limit).collect();
    let mut out = Vec::new();
    for (meta, dir, source, score) in top {
        let mut entry = meta_to_entry(&meta, source, Some(score));
        if include_content {
            if let Ok(full) = get_entry_by_id_single(&dir, &meta.id) {
                entry.content = full.content;
            }
        }
        out.push(entry);
    }
    if let Some(logger) = log {
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("search"));
        fields.insert("query".to_string(), serde_json::json!(query));
        fields.insert("result_count".to_string(), serde_json::json!(out.len()));
        let _ = logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory read".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
    }
    Ok(out)
}

fn meta_to_entry(m: &MemoryMeta, source: String, score: Option<u64>) -> MemoryEntry {
    MemoryEntry {
        id: m.id.clone(),
        content: String::new(),
        category: m.category.clone(),
        keywords: m.keywords.clone(),
        subject: m.subject.clone(),
        timestamp: m.timestamp.clone(),
        usage_count: m.usage_count,
        memory_dir: m.memory_dir.clone(),
        project_root: m.project_root.clone(),
        source: Some(source),
        score,
    }
}

/// 単一ディレクトリから ID で取得（entries/<id>.json を読む）
fn get_entry_by_id_single(dir: &Path, id: &str) -> Result<MemoryEntry, Error> {
    let path = dir.join(ENTRIES_DIR).join(format!("{}.json", id));
    if !path.exists() {
        return Err(Error::io_msg(format!("memory not found: {}", id)));
    }
    let s = std::fs::read_to_string(&path)
        .map_err(|e| Error::io_msg(format!("read {}: {}", path.display(), e)))?;
    let e: MemoryEntry =
        serde_json::from_str(&s).map_err(|err| Error::io_msg(format!("parse {}: {}", path.display(), err)))?;
    Ok(e)
}

/// プロジェクト優先で ID に一致するエントリを取得。
/// log に Some を渡すとメモリ読み込みをログに記録する。
pub fn get_entry_by_id(
    project_dir: Option<&Path>,
    global_dir: &Path,
    id: &str,
    log: Option<&dyn Log>,
) -> Result<MemoryEntry, Error> {
    if let Some(d) = project_dir {
        if let Ok(mut e) = get_entry_by_id_single(d, id) {
            e.source = Some("project".to_string());
            if let Some(logger) = log {
                let mut fields = BTreeMap::new();
                fields.insert("operation".to_string(), serde_json::json!("get"));
                fields.insert("memory_id".to_string(), serde_json::json!(id));
                fields.insert("source".to_string(), serde_json::json!("project"));
                let _ = logger.log(&LogRecord {
                    ts: now_iso8601(),
                    level: LogLevel::Info,
                    message: "memory read".to_string(),
                    layer: Some("adapter".to_string()),
                    kind: Some("memory".to_string()),
                    fields: Some(fields),
                });
            }
            return Ok(e);
        }
    }
    let mut e = get_entry_by_id_single(global_dir, id)?;
    e.source = Some("global".to_string());
    if let Some(logger) = log {
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("get"));
        fields.insert("memory_id".to_string(), serde_json::json!(id));
        fields.insert("source".to_string(), serde_json::json!("global"));
        let _ = logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory read".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MemoryEntry;

    #[test]
    fn test_init_and_save_and_load() {
        let dir = std::env::temp_dir().join("memory_storage_test");
        let _ = std::fs::remove_dir_all(&dir);
        init_memory_dir(&dir).unwrap();
        assert!(dir.join(METADATA_FILENAME).exists());
        assert!(dir.join(ENTRIES_DIR).is_dir());

        let entry = MemoryEntry::new(
            "",
            "test content",
            "general",
            vec!["kw1".to_string(), "kw2".to_string()],
            "subject",
            "2025-01-01T00:00:00Z",
        );
        let id = save_entry(&dir, &entry, None).unwrap();
        assert!(!id.is_empty());
        assert!(dir.join(ENTRIES_DIR).join(format!("{}.json", id)).exists());

        let metas = load_metadata(&dir, None).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].subject, "subject");

        let found = get_entry_by_id(Some(&dir), &dir, &id, None).unwrap();
        assert_eq!(found.content, "test content");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
