//! replayable shell log の読み込み（filesystem I/O は adapter 側）。

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use aish_replay::LogEvent;

#[derive(Debug, thiserror::Error)]
pub enum ReplaySourceError {
    #[error("replay log unreadable: {0}")]
    LogRead(String),
    #[error("replay log invalid line: {0}")]
    InvalidLine(String),
}

pub fn load_replay_events(path: &Path) -> Result<Vec<LogEvent>, ReplaySourceError> {
    let file = File::open(path).map_err(|e| ReplaySourceError::LogRead(e.to_string()))?;
    let reader = BufReader::new(file);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|e| ReplaySourceError::LogRead(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event: LogEvent = serde_json::from_str(&line)
            .map_err(|e| ReplaySourceError::InvalidLine(e.to_string()))?;
        events.push(event);
    }
    Ok(events)
}
