//! メモリの list / get / remove をログに記録する MemoryRepository のラッパ

use crate::domain::{MemoryEntry, MemoryListEntry};
use crate::ports::outbound::MemoryRepository;
use common::ports::outbound::{Log, LogLevel, LogRecord, now_iso8601};
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct LoggingMemoryRepository {
    inner: Arc<dyn MemoryRepository>,
    log: Arc<dyn Log>,
}

impl LoggingMemoryRepository {
    pub fn new(inner: Arc<dyn MemoryRepository>, log: Arc<dyn Log>) -> Self {
        Self { inner, log }
    }
}

impl MemoryRepository for LoggingMemoryRepository {
    fn resolve(&self) -> Result<(Option<std::path::PathBuf>, std::path::PathBuf), common::error::Error> {
        self.inner.resolve()
    }

    fn list(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
    ) -> Result<Vec<MemoryListEntry>, common::error::Error> {
        let out = self.inner.list(project_dir, global_dir)?;
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("list"));
        fields.insert("count".to_string(), serde_json::json!(out.len()));
        let _ = self.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory read".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
        Ok(out)
    }

    fn get(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<MemoryEntry, common::error::Error> {
        let out = self.inner.get(project_dir, global_dir, id)?;
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("get"));
        fields.insert("memory_id".to_string(), serde_json::json!(id));
        let _ = self.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory read".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
        Ok(out)
    }

    fn remove(
        &self,
        project_dir: Option<&std::path::Path>,
        global_dir: &std::path::Path,
        id: &str,
    ) -> Result<(), common::error::Error> {
        self.inner.remove(project_dir, global_dir, id)?;
        let mut fields = BTreeMap::new();
        fields.insert("operation".to_string(), serde_json::json!("remove"));
        fields.insert("memory_id".to_string(), serde_json::json!(id));
        let _ = self.log.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "memory write".to_string(),
            layer: Some("adapter".to_string()),
            kind: Some("memory".to_string()),
            fields: Some(fields),
        });
        Ok(())
    }
}
