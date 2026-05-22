//! JSONL ファイルログアダプタ。

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use crate::domain::LogEvent;
use crate::ports::outbound::{LogError, SessionLog};

pub struct JsonlFileLog {
    path: PathBuf,
}

impl JsonlFileLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl SessionLog for JsonlFileLog {
    fn append(&mut self, event: &LogEvent) -> Result<(), LogError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| LogError::Write(e.to_string()))?;
        }
        let line = serde_json::to_string(event).map_err(|e| LogError::Write(e.to_string()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(|e| LogError::Write(e.to_string()))?;
        writeln!(file, "{line}").map_err(|e| LogError::Write(e.to_string()))?;
        Ok(())
    }
}
