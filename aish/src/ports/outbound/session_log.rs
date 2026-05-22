//! セッションログ outbound port。

use crate::domain::LogEvent;

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("failed to write log: {0}")]
    Write(String),
}

/// JSONL へイベントを追記する。
pub trait SessionLog {
    fn append(&mut self, event: &LogEvent) -> Result<(), LogError>;
}
