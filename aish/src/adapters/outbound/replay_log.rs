//! JSONL ログファイルの読み取り。

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::domain::LogEvent;

#[derive(Debug, thiserror::Error)]
pub enum ReplayLogReadError {
    #[error("failed to read log: {0}")]
    Read(String),
    #[error("invalid log line: {0}")]
    InvalidLine(String),
}

pub fn read_log_events(path: &Path) -> Result<Vec<LogEvent>, ReplayLogReadError> {
    let file = File::open(path).map_err(|e| ReplayLogReadError::Read(e.to_string()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| ReplayLogReadError::Read(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event: LogEvent = serde_json::from_str(&line)
            .map_err(|e| ReplayLogReadError::InvalidLine(e.to_string()))?;
        events.push(event);
    }
    Ok(events)
}
